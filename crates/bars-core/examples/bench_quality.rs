//! Качество и скорость поиска: без rerank, rerank на процессоре и rerank на видеокарте.
//! `cargo run --release -p bars-core --example bench_quality -- <копия index.sqlite>`.
//! Эталон — пары «вопрос → документ, в котором ответ»; документ из `docs/` проекта.

use std::path::{Path, PathBuf};
use std::time::Instant;

use bars_core::models::{Accelerator, BuiltinEmbedder, BuiltinReranker, Reranker};
use bars_core::search::{SearchRequest, SearchSettings, search};
use bars_core::storage::{Index, Journal};

const CASES: &[(&str, &str)] = &[
    (
        "как bars режет документы на чанки по заголовкам",
        "indexing/chunking.md",
    ),
    (
        "как работает гибридный поиск и слияние RRF",
        "search/hybrid-search.md",
    ),
    (
        "какие инструменты MCP доступны агенту",
        "agent-tools/mcp-tools.md",
    ),
    ("команды CLI bars-cli", "agent-tools/cli.md"),
    (
        "как хранятся ключи провайдеров и секреты",
        "architecture/secrets.md",
    ),
    ("схема SQLite индекса и таблицы", "architecture/schema.md"),
    (
        "как воркер ставит проекты в очередь индексации",
        "architecture/worker.md",
    ),
    (
        "установщик NSIS и обновление на месте",
        "distribution/installer.md",
    ),
    (
        "автозапуск воркера при входе в систему",
        "distribution/autostart.md",
    ),
    (
        "подключение к внешним embed-моделям по HTTP",
        "models/external-models.md",
    ),
    (
        "какие локальные модели есть в каталоге и как они скачиваются",
        "models/local-models.md",
    ),
    (
        "как регистрируется проект и что такое корень проекта",
        "projects/projects.md",
    ),
    (
        "порядок настроек и файл settings.toml",
        "settings/settings.md",
    ),
    (
        "состояния окна при ошибках поиска",
        "window/error-states.md",
    ),
    (
        "пустые состояния окна, когда нет проектов",
        "window/empty-states.md",
    ),
    ("прогресс индексации в окне", "window/indexing-progress.md"),
    (
        "согласие пользователя на переиндексацию после смены модели",
        "window/reindex-consent.md",
    ),
    (
        "как слежение за файлами обновляет индекс",
        "indexing/updates.md",
    ),
    (
        "как установить bars в Claude Code и Codex",
        "clients/clients.md",
    ),
    (
        "как вести журнал сессии log.md и handoff.md",
        "docs-convention/session-log.md",
    ),
    (
        "раскладка папки docs и Index.md",
        "docs-convention/layout.md",
    ),
    (
        "протокол между окном и воркером",
        "architecture/protocol.md",
    ),
    (
        "косинусная близость и перебор векторов",
        "architecture/vector-search.md",
    ),
    ("стек Rust Tauri fastembed", "architecture/stack.md"),
];

fn hit_rank(paths: &[String], suffix: &str) -> Option<usize> {
    paths
        .iter()
        .position(|p| p.replace('\\', "/").ends_with(suffix))
}

#[derive(Default)]
struct Stats {
    top1: usize,
    top3: usize,
    top5: usize,
    mrr: f64,
    secs: Vec<f64>,
    top_scores: Vec<f32>,
}

impl Stats {
    fn add(&mut self, rank: Option<usize>, secs: f64, top: f32) {
        if let Some(r) = rank {
            self.top1 += (r == 0) as usize;
            self.top3 += (r < 3) as usize;
            self.top5 += (r < 5) as usize;
            self.mrr += 1.0 / (r as f64 + 1.0);
        }
        self.secs.push(secs);
        self.top_scores.push(top);
    }
    fn print(&self, name: &str) {
        let n = CASES.len() as f64;
        let mut s = self.secs.clone();
        s.sort_by(|a, b| a.total_cmp(b));
        let med = s[s.len() / 2];
        let min = self.top_scores.iter().cloned().fold(f32::MAX, f32::min);
        let max = self.top_scores.iter().cloned().fold(f32::MIN, f32::max);
        println!(
            "{name:<16} top1 {:>5.1}%  top3 {:>5.1}%  top5 {:>5.1}%  MRR {:.3}  медиана {:.2} с  оценка лидера {:.3}…{:.3}",
            100.0 * self.top1 as f64 / n,
            100.0 * self.top3 as f64 / n,
            100.0 * self.top5 as f64 / n,
            self.mrr / n,
            med,
            min,
            max
        );
    }
}

fn main() {
    let index_copy = PathBuf::from(std::env::args().nth(1).expect("путь к копии index.sqlite"));
    let index = Index::open(&index_copy, Journal::Delete).unwrap();
    let models = PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(".bars/models");
    let mut embedder = BuiltinEmbedder::bge_m3(&models, Accelerator::Auto).unwrap();
    eprintln!("embed: {:?}", embedder.device());
    let mut cpu = BuiltinReranker::bge_reranker_v2_m3(&models, Accelerator::Cpu).unwrap();
    let mut gpu = BuiltinReranker::bge_reranker_v2_m3(&models, Accelerator::Auto).unwrap();

    let settings = SearchSettings {
        results: 10,
        results_reranked: 10,
        min_score_rerank: 0.0,
        min_score_vector: 0.0,
        ..SearchSettings::default()
    };
    // Прогрев, чтобы шейдеры DirectML не попали в замер.
    let warm = SearchRequest {
        query: "прогрев".into(),
        ..Default::default()
    };
    let _ = search(&index, &mut embedder, Some(&mut gpu), &settings, &warm);
    let _ = search(&index, &mut embedder, Some(&mut cpu), &settings, &warm);

    let (mut none, mut s_cpu, mut s_gpu) = (Stats::default(), Stats::default(), Stats::default());
    let mut diffs: Vec<f32> = Vec::new();
    for (query, suffix) in CASES {
        let request = SearchRequest {
            query: query.to_string(),
            ..Default::default()
        };
        let mut run = |rr: Option<&mut dyn Reranker>, st: &mut Stats| {
            let t = Instant::now();
            let r = search(&index, &mut embedder, rr, &settings, &request).unwrap();
            let secs = t.elapsed().as_secs_f64();
            if let Some(e) = &r.rerank_error {
                eprintln!("rerank error: {e}");
            }
            let paths: Vec<String> = r.hits.iter().map(|h| h.path.clone()).collect();
            let top = r.hits.first().map_or(0.0, |h| h.score);
            st.add(hit_rank(&paths, suffix), secs, top);
            r
        };
        let a = run(None, &mut none);
        let b = run(Some(&mut cpu), &mut s_cpu);
        let c = run(Some(&mut gpu), &mut s_gpu);
        // Порядок без rerank против шкалы: сколько пар соседей идёт «против» косинуса.
        let inversions = a
            .hits
            .windows(2)
            .filter(|w| w[0].score < w[1].score)
            .count();
        let d = b.hits.first().map_or(0.0, |x| x.score) - c.hits.first().map_or(0.0, |x| x.score);
        diffs.push(d.abs());
        println!(
            "{:<60} без:{:?} cpu:{:?} gpu:{:?}  инверсий шкалы без rerank: {inversions}",
            query,
            hit_rank(
                &a.hits.iter().map(|h| h.path.clone()).collect::<Vec<_>>(),
                suffix
            ),
            hit_rank(
                &b.hits.iter().map(|h| h.path.clone()).collect::<Vec<_>>(),
                suffix
            ),
            hit_rank(
                &c.hits.iter().map(|h| h.path.clone()).collect::<Vec<_>>(),
                suffix
            ),
        );
    }
    println!();
    none.print("без rerank");
    s_cpu.print("rerank CPU");
    s_gpu.print("rerank GPU");
    println!(
        "расхождение оценки лидера CPU↔GPU: среднее {:.4}, максимум {:.4}",
        diffs.iter().sum::<f32>() / diffs.len() as f32,
        diffs.iter().cloned().fold(0.0, f32::max)
    );

    // Развёртка: сколько кандидатов отдавать реранку, и порядок без rerank по косинусу.
    println!();
    let mut by_cos = Stats::default();
    for (query, suffix) in CASES {
        let request = SearchRequest {
            query: query.to_string(),
            ..Default::default()
        };
        let t = Instant::now();
        let r = search(&index, &mut embedder, None, &settings, &request).unwrap();
        let secs = t.elapsed().as_secs_f64();
        let mut hits = r.hits;
        hits.sort_by(|a, b| b.score.total_cmp(&a.score));
        let paths: Vec<String> = hits.iter().map(|h| h.path.clone()).collect();
        by_cos.add(
            hit_rank(&paths, suffix),
            secs,
            hits.first().map_or(0.0, |h| h.score),
        );
    }
    by_cos.print("без rerank, по cos");
    for k in [3usize, 5, 8, 10] {
        let mut st = Stats::default();
        for (query, suffix) in CASES {
            let request = SearchRequest {
                query: query.to_string(),
                rerank_candidates: Some(k),
                ..Default::default()
            };
            let t = Instant::now();
            let r = search(&index, &mut embedder, Some(&mut gpu), &settings, &request).unwrap();
            let secs = t.elapsed().as_secs_f64();
            let paths: Vec<String> = r.hits.iter().map(|h| h.path.clone()).collect();
            st.add(
                hit_rank(&paths, suffix),
                secs,
                r.hits.first().map_or(0.0, |h| h.score),
            );
        }
        st.print(&format!("rerank GPU, K={k}"));
    }
    let _ = Path::new("");
}
