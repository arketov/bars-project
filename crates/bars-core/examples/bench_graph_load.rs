//! Замер стоимости загрузки графа и построения по слоям на корпусе заданного размера (ISS-77).
//!
//! `cargo run --release -p bars-core --example bench_graph_load -- <папка-docs> <вывод.json> [<чанков>…]`
//!
//! Корпус — Markdown-файлы из `<папка-docs>` (естественные 5 000 чанков из исследования CPU-поиска),
//! разложенные по одноразовому проекту во временном каталоге порциями, пока число чанков не дойдёт
//! до цели. Если файлов не хватило, добавляются копии всего набора с меткой в строках, чтобы тексты
//! чанков различались. Векторы — детерминированный хеш слов (1024 измерения), без модели: близость
//! соседей не похожа на настоящую, но число узлов, рёбер и доказательств и размер данных от неё
//! почти не зависят. Временный каталог удаляется в конце. Индексы пользователя не открываются.

use bars_core::graph::similarity::{KnnOptions, VectorNode, build_knn_observed};
use bars_core::graph::{BuildLayer, GraphRequest, Operation, build_index_graph_layer, query, store};
use bars_core::indexing::chunking::ChunkingParams;
use bars_core::indexing::indexer::{IndexSettings, UpdateOutcome, update_index};
use bars_core::models::{EmbedError, Embedder, ModelFingerprint};
use bars_core::storage::{Index, Journal, index_path};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const DIM: usize = 1024;
const LOAD_RUNS: usize = 15;

struct HashEmbedder(ModelFingerprint);

impl Embedder for HashEmbedder {
    fn fingerprint(&self) -> &ModelFingerprint {
        &self.0
    }
    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        Ok(texts.iter().map(|t| hashed_vector(t)).collect())
    }
}

fn hash64(bytes: &[u8]) -> u64 {
    let h = blake3::hash(bytes);
    u64::from_le_bytes(h.as_bytes()[..8].try_into().unwrap())
}

/// Мешок слов в 1024 измерениях; слова-метки копий (`кк…`) не учитываются, к ним добавлен шум от всего текста.
fn hashed_vector(text: &str) -> Vec<f32> {
    let mut v = vec![0f32; DIM];
    for word in text.split_whitespace() {
        let word = word.to_lowercase();
        if word.starts_with("кк") {
            continue;
        }
        let h = hash64(word.as_bytes());
        v[(h % DIM as u64) as usize] += 1.0;
        v[((h >> 20) % DIM as u64) as usize] -= 0.5;
    }
    let noise = hash64(text.as_bytes());
    for (i, x) in v.iter_mut().enumerate() {
        *x += (((noise >> (i % 48)) & 0xff) as f32 / 255.0 - 0.5) * 0.2;
    }
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
    v.iter_mut().for_each(|x| *x /= norm);
    v
}

fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 1000.0 * 10.0).round() / 10.0
}

fn stats(mut runs: Vec<Duration>) -> Value {
    runs.sort();
    let at = |q: f64| ms(runs[((runs.len() - 1) as f64 * q).round() as usize]);
    json!({"runs": runs.len(), "min_ms": at(0.0), "median_ms": at(0.5), "p95_ms": at(0.95), "max_ms": at(1.0)})
}

fn list_md(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            list_md(&path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
}

fn tagged(text: &str, tag: &str) -> String {
    text.lines()
        .map(|line| {
            let t = line.trim_start();
            if t.is_empty() || t.starts_with("```") || t.starts_with("---") || t.starts_with('|') {
                line.to_string()
            } else {
                format!("{line} {tag}")
            }
        })
        .collect::<Vec<_>>()
        .join("
")
}

fn table_bytes(index: &Index, table: &str) -> (i64, i64) {
    index
        .conn()
        .query_row(
            &format!("SELECT COUNT(*), COALESCE(SUM(LENGTH(data)),0) FROM {table}"),
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
}

fn rows(index: &Index, sql: &str) -> Vec<String> {
    let mut stmt = index.conn().prepare(sql).unwrap();
    stmt.query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn measure(source: &Path, target_chunks: usize, work: &Path) -> Value {
    let project = work.join(format!("p{target_chunks}"));
    let docs = project.join("docs");
    let mut embedder = HashEmbedder(ModelFingerprint {
        source: "bench".into(),
        model: "hashed-words".into(),
        dim: DIM,
        doc_prefix: String::new(),
    });
    let settings = IndexSettings {
        chunking: ChunkingParams::default(),
        docs_root: "docs".into(),
    };
    std::fs::create_dir_all(project.join(".bars")).unwrap();
    let mut index = Index::open(&index_path(&project), Journal::Delete).unwrap();
    let mut files = Vec::new();
    list_md(source, &mut files);
    let (mut pass, mut next) = (0usize, 0usize);
    while index.chunk_count().unwrap() < target_chunks {
        for _ in 0..40 {
            if next == files.len() {
                next = 0;
                pass += 1;
            }
            let file = &files[next];
            next += 1;
            let relative = file.strip_prefix(source).unwrap();
            let relative = if pass == 0 {
                relative.to_path_buf()
            } else {
                Path::new(&format!("copy{pass}")).join(relative)
            };
            let target = docs.join(relative);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            let text = std::fs::read_to_string(file).unwrap();
            let text = if pass == 0 { text } else { tagged(&text, &format!("кк{pass}")) };
            std::fs::write(target, text).unwrap();
        }
        let UpdateOutcome::Updated(report) =
            update_index(&project, &mut index, &settings, &mut embedder).unwrap()
        else {
            panic!("индекс требует переиндексации");
        };
        assert!(report.problems.is_empty(), "{:?}", report.problems);
    }
    let chunks = index.chunk_count().unwrap();
    let documents = index.document_count().unwrap();

    // Построение по слоям: каждое чтение документов и разбор общие, различается только близость.
    let mut build = serde_json::Map::new();
    for (name, layer) in [
        ("all_fresh", BuildLayer::All),
        ("structure", BuildLayer::Structure),
        ("semantic", BuildLayer::Semantic),
        ("similarity", BuildLayer::Similarity),
    ] {
        let started = Instant::now();
        let status = build_index_graph_layer(&project, &mut index, layer, || false).unwrap();
        build.insert(
            name.into(),
            json!({"ms": ms(started.elapsed()), "state": format!("{:?}", status.state)}),
        );
    }
    // Один только kNN по тем же векторам с тем же пределом, что у построения графа; полный расчёт без предела измерен в knowledge/chunk-similarity.md.
    let nodes: Vec<VectorNode> = {
        let mut stmt = index
            .conn()
            .prepare("SELECT embed_hash FROM chunks ORDER BY id")
            .unwrap();
        let hashes: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        hashes
            .into_iter()
            .enumerate()
            .filter_map(|(i, h)| {
                index.vector(&h).unwrap().map(|vector| VectorNode {
                    id: format!("n{i}"),
                    vector,
                })
            })
            .collect()
    };
    let started = Instant::now();
    let capped = build_knn_observed(
        &nodes,
        KnnOptions {
            k: 5,
            min_cosine: 0.5,
            max_comparisons: 1_000_000,
        },
        || false,
        |_, _| {},
    )
    .unwrap();
    let knn_capped = json!({"ms": ms(started.elapsed()), "rows_done": capped.neighbors.len(), "rows_total": capped.total_nodes, "stop_reason": format!("{:?}", capped.stop_reason)});
    drop(index);

    // Загрузка так, как её делает воркер на каждый запрос: открыть только на чтение и разобрать весь набор.
    let path = index_path(&project);
    let ro = Index::open_read_only(&path).unwrap();
    let sizes: Value = ["graph_sources", "graph_nodes", "graph_edges", "graph_evidence"]
        .iter()
        .map(|t| {
            let (count, bytes) = table_bytes(&ro, t);
            (t.to_string(), json!({"rows": count, "json_bytes": bytes}))
        })
        .collect::<serde_json::Map<_, _>>()
        .into();
    let mut open_runs = Vec::new();
    let mut load_runs = Vec::new();
    for _ in 0..LOAD_RUNS {
        let t = Instant::now();
        let index = Index::open_read_only(&path).unwrap();
        open_runs.push(t.elapsed());
        let t = Instant::now();
        let data = store::load(index.conn()).unwrap();
        load_runs.push(t.elapsed());
        drop(data);
    }
    // Разбор по частям: чтение строк SQLite, разбор JSON, проверка целостности.
    let (mut sql_runs, mut parse_runs, mut validate_runs) = (vec![], vec![], vec![]);
    for _ in 0..LOAD_RUNS {
        let t = Instant::now();
        let s = rows(&ro, "SELECT data FROM graph_sources ORDER BY path");
        let n = rows(&ro, "SELECT data FROM graph_nodes ORDER BY id");
        let e = rows(&ro, "SELECT data FROM graph_edges ORDER BY id");
        let v = rows(&ro, "SELECT data FROM graph_evidence ORDER BY id");
        sql_runs.push(t.elapsed());
        let t = Instant::now();
        let parsed = (
            s.iter().map(|x| serde_json::from_str::<bars_graph::SourceDocument>(x).unwrap()).count(),
            n.iter().map(|x| serde_json::from_str::<bars_graph::Node>(x).unwrap()).count(),
            e.iter().map(|x| serde_json::from_str::<bars_graph::Edge>(x).unwrap()).count(),
            v.iter().map(|x| serde_json::from_str::<bars_graph::Evidence>(x).unwrap()).count(),
        );
        parse_runs.push(t.elapsed());
        std::hint::black_box(parsed);
    }
    let data = store::load(ro.conn()).unwrap();
    for _ in 0..LOAD_RUNS {
        let t = Instant::now();
        store::validate(&data).unwrap();
        validate_runs.push(t.elapsed());
    }
    let counts = json!({"sources": data.sources.len(), "nodes": data.nodes.len(), "edges": data.edges.len(), "evidence": data.evidence.len(),
        "similar_edges": data.edges.iter().filter(|e| e.origin == bars_graph::Origin::Similarity).count()});

    // Остальная часть запроса: те же операции над уже загруженным набором.
    let mut operations = serde_json::Map::new();
    let explore = GraphRequest {
        operation: Operation::Explore,
        whole_scope: true,
        ..Default::default()
    };
    for (name, request) in [
        ("graph_default", GraphRequest::default()),
        ("explore_whole_scope", explore),
    ] {
        let mut runs = Vec::new();
        let mut note = Value::Null;
        for _ in 0..LOAD_RUNS {
            let t = Instant::now();
            match query::execute(&data, &request, || false) {
                Ok(_) => runs.push(t.elapsed()),
                Err(e) => {
                    note = json!(e.to_string());
                    break;
                }
            }
        }
        operations.insert(
            name.into(),
            if runs.is_empty() {
                json!({"error": note})
            } else {
                stats(runs)
            },
        );
    }
    json!({
        "target_chunks": target_chunks, "chunks": chunks, "documents": documents, "files": next + pass * files.len(), "full_copies": pass,
        "graph": counts, "table_sizes": sizes,
        "build_ms": build, "knn_capped_1m": knn_capped,
        "load": {"open_read_only": stats(open_runs), "store_load": stats(load_runs),
                 "sql_read": stats(sql_runs), "json_parse": stats(parse_runs), "validate": stats(validate_runs)},
        "query_after_load": operations,
    })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("bench_graph_load <проект-источник> <вывод.json> [<чанков>…]");
        std::process::exit(2);
    }
    let source = PathBuf::from(&args[0]);
    let targets: Vec<usize> = if args.len() > 2 {
        args[2..].iter().map(|a| a.parse().unwrap()).collect()
    } else {
        vec![1250, 2500, 5000, 10000]
    };
    let work = std::env::temp_dir().join("bars-graph-load");
    assert!(!work.exists(), "{} уже существует", work.display());
    std::fs::create_dir_all(&work).unwrap();
    let results: Vec<Value> = targets.iter().map(|t| measure(&source, *t, &work)).collect();
    let report = json!({
        "date": "2026-10-24", "method": "natural corpus files, then tagged copies; hashed-word vectors; release build",
        "load_runs": LOAD_RUNS, "sizes": results,
    });
    std::fs::write(&args[1], serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    std::fs::remove_dir_all(&work).unwrap();
}
