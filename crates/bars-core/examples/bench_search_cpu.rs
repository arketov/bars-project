//! НФТ-2: естественный корпус, повторно используемые настоящие векторы, полный CPU-поиск.
//! Протокол: docs/knowledge/inference-speed.md, «Естественные 5 000 чанков».
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use bars_core::indexing::chunking::{ChunkingParams, chunk_document};
use bars_core::models::{
    Accelerator, BuiltinEmbedder, Device, EmbedError, Embedder, ModelFingerprint,
    bge_m3_fingerprint, is_downloaded,
};
use bars_core::search::{ScoreKind, SearchRequest, SearchSettings, search};
use bars_core::storage::{DocumentRecord, Index, Journal};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[cfg(windows)]
#[allow(dead_code)]
#[path = "bench_memory.rs"]
mod memory_probe;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Deserialize)]
struct Inputs {
    roots: Vec<PathBuf>,
    seeds: Vec<PathBuf>,
    queries: Vec<String>,
}
#[derive(Serialize, Deserialize)]
struct Document {
    root: PathBuf,
    path: String,
    source: String,
    hash: String,
    selected: usize,
}
#[derive(Serialize, Deserialize)]
struct Corpus {
    requested: usize,
    available: usize,
    selected: usize,
    documents: Vec<Document>,
}

fn digest(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}
fn read<T: serde::de::DeserializeOwned>(path: &str) -> Result<T> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn save(path: &str, value: &impl Serialize) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    Ok(())
}
fn markdown(dir: &Path, relative: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries = fs::read_dir(dir.join(relative))?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(format!("ссылка в корпусе: {}", entry.path().display()).into());
        }
        let path = relative.join(entry.file_name());
        if kind.is_dir() {
            markdown(dir, &path, paths)?;
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("md"))
            && !entry
                .file_name()
                .eq_ignore_ascii_case("BARS_INSTRUCTION.md")
        {
            paths.push(path);
        }
    }
    Ok(())
}
fn freeze(inputs: &Inputs, output: &str) -> Result<()> {
    let mut corpus = Corpus {
        requested: 5000,
        available: 0,
        selected: 0,
        documents: Vec::new(),
    };
    let mut paths = HashSet::new();
    for root in &inputs.roots {
        let mut files = Vec::new();
        markdown(root, Path::new(""), &mut files)?;
        for file in files {
            let path = format!("docs/{}", file.to_string_lossy().replace('\\', "/"));
            if !paths.insert(path.to_lowercase()) {
                continue;
            }
            let source = fs::read_to_string(root.join(&file))?;
            let count = chunk_document(&path, &source, &ChunkingParams::default())
                .chunks
                .len();
            corpus.available += count;
            let selected = count.min(corpus.requested - corpus.selected);
            if selected == 0 {
                continue;
            }
            corpus.selected += selected;
            corpus.documents.push(Document {
                root: root.clone(),
                path,
                hash: digest(source.as_bytes()),
                source,
                selected,
            });
        }
    }
    eprintln!(
        "естественных чанков: {}; выбрано: {}; документов: {}",
        corpus.available,
        corpus.selected,
        corpus.documents.len()
    );
    save(output, &corpus)
}
fn validate(corpus: &Corpus) -> Result<()> {
    if corpus.selected != 5000 || corpus.requested != 5000 {
        return Err("нужно ровно 5000 естественных чанков".into());
    }
    let mut hashes = HashSet::new();
    let mut paths = HashSet::new();
    let mut count = 0;
    for doc in &corpus.documents {
        if digest(doc.source.as_bytes()) != doc.hash || !paths.insert(doc.path.to_lowercase()) {
            return Err("повреждён снимок или повторён путь".into());
        }
        let chunked = chunk_document(&doc.path, &doc.source, &ChunkingParams::default());
        if doc.selected == 0 || doc.selected > chunked.chunks.len() {
            return Err("неверное число выбранных чанков".into());
        }
        for chunk in chunked.chunks.iter().take(doc.selected) {
            if !hashes.insert(chunk.embed_hash.clone()) {
                return Err("повторён embed_hash".into());
            }
            count += 1;
        }
    }
    if count != 5000 {
        return Err("число чанков не совпало с манифестом".into());
    }
    Ok(())
}
fn model(models: &Path, accelerator: Accelerator) -> Result<BuiltinEmbedder> {
    if std::env::var_os("HF_HOME").is_some() || !is_downloaded(models, "BAAI/bge-m3") {
        return Err(
            "требуется готовый кэш bge-m3, без HF_HOME; скачивание запрещено стендом".into(),
        );
    }
    Ok(BuiltinEmbedder::bge_m3(models, accelerator)?)
}
fn valid_vector(vector: &[f32]) -> bool {
    vector.len() == 1024 && vector.iter().all(|v| v.is_finite()) && vector.iter().any(|v| *v != 0.0)
}
fn cache_plan(corpus: &Corpus, inputs: &Inputs, output: &str) -> Result<()> {
    validate(corpus)?;
    let seeds = inputs
        .seeds
        .iter()
        .map(|p| Index::open_read_only(p))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let fingerprint = bge_m3_fingerprint();
    for seed in &seeds {
        seed.conn().execute_batch("BEGIN")?;
        if seed.stored_fingerprint()?.as_ref() != Some(&fingerprint) {
            return Err("несовместимая модель seed".into());
        }
    }
    let mut reused = 0;
    let mut pending = Vec::new();
    let mut documents = Vec::new();
    for doc in &corpus.documents {
        let chunked = chunk_document(&doc.path, &doc.source, &ChunkingParams::default());
        let mut chunks = Vec::new();
        for chunk in chunked.chunks.iter().take(doc.selected) {
            let mut cached = None;
            for (i, seed) in seeds.iter().enumerate() {
                if let Some(vector) = seed.vector(&chunk.embed_hash)? {
                    if !valid_vector(&vector) {
                        return Err("некорректный seed-вектор".into());
                    }
                    cached = Some(i);
                    break;
                }
            }
            if cached.is_some() {
                reused += 1;
            } else {
                pending.push(json!({"path":doc.path,"ordinal":chunk.ordinal,"embed_hash":chunk.embed_hash,"chars":chunk.embed_text.chars().count()}));
            }
            chunks.push(json!({"ordinal":chunk.ordinal,"embed_hash":chunk.embed_hash,"first_line":chunk.first_line,"last_line":chunk.last_line,"seed":cached}));
        }
        documents.push(json!({"root":doc.root,"path":doc.path,"hash":doc.hash,"selected":doc.selected,"chunks":chunks}));
    }
    save(
        output,
        &json!({"status":"planned-no-model","available":corpus.available,"chunks":corpus.selected,"documents":documents,"reused":reused,"pending":pending,"corpus_blake3":digest(&serde_json::to_vec(corpus)?),"seeds":inputs.seeds,"fingerprint":format!("{fingerprint:?}")}),
    )
}
fn guarded(output: &str, reserve: u64, action: impl FnOnce() -> Result<()>) -> Result<()> {
    #[cfg(windows)]
    {
        use memory_probe::windows::{Log, Monitor};
        let log = Log::create(Path::new(&format!("{output}.memory.jsonl")))?;
        log.reserve(reserve)?;
        log.emit(
            "config",
            "search_cpu",
            json!({"startup_commit_gib":reserve,"stop_commit_gib":1,"interval_ms":100}),
        );
        let monitor = Monitor::start(log.clone());
        let result = action();
        log.emit(
            "end",
            "search_cpu",
            json!({"ok":result.is_ok(),"error":result.as_ref().err().map(ToString::to_string)}),
        );
        result.and(monitor.stop())
    }
    #[cfg(not(windows))]
    {
        let _ = (output, reserve, action);
        Err("аппаратный прогон требует Windows guard; freeze/cache-plan доступны отдельно".into())
    }
}
/// `cpu` — запасное устройство подготовки (OQ-196); измеряемый путь от него не зависит.
fn prepare(corpus: &Corpus, inputs: &Inputs, output: &str, models: &Path, cpu: bool) -> Result<()> {
    validate(corpus)?;
    if Path::new(output).exists() {
        return Err("индекс уже существует".into());
    }
    let fingerprint = bge_m3_fingerprint();
    let seeds = inputs
        .seeds
        .iter()
        .map(|p| Index::open_read_only(p))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for seed in &seeds {
        seed.conn().execute_batch("BEGIN")?;
        if seed.stored_fingerprint()?.as_ref() != Some(&fingerprint) {
            return Err("несовместимая модель seed".into());
        }
    }
    // Не удерживать снимки рабочих индексов во время загрузки модели и инференса.
    let mut cached_vectors = HashMap::new();
    for doc in &corpus.documents {
        let chunked = chunk_document(&doc.path, &doc.source, &ChunkingParams::default());
        for chunk in chunked.chunks.iter().take(doc.selected) {
            for seed in &seeds {
                if let Some(vector) = seed.vector(&chunk.embed_hash)? {
                    if !valid_vector(&vector) {
                        return Err("некорректный seed-вектор".into());
                    }
                    cached_vectors.insert(chunk.embed_hash.clone(), vector);
                    break;
                }
            }
        }
    }
    drop(seeds);
    let mut index = Index::open(Path::new(output), Journal::Delete)?;
    index.write_fingerprint(&fingerprint)?;
    let mut embedder = None;
    let (mut reused, mut computed) = (0, 0);
    for doc in &corpus.documents {
        let mut chunked = chunk_document(&doc.path, &doc.source, &ChunkingParams::default());
        chunked.chunks.truncate(doc.selected);
        let mut vectors = Vec::new();
        for chunk in &chunked.chunks {
            let vector = if let Some(vector) = cached_vectors.remove(&chunk.embed_hash) {
                reused += 1;
                vector
            } else {
                if embedder.is_none() {
                    eprintln!("требуется настоящий embed; reused={reused}, computed={computed}");
                    let (accelerator, device) = if cpu {
                        (Accelerator::Cpu, Device::Cpu)
                    } else {
                        (Accelerator::Auto, Device::DirectMl)
                    };
                    let loaded = model(models, accelerator)?;
                    if loaded.device() != device {
                        return Err(format!("подготовка требует {device:?} без fallback").into());
                    }
                    embedder = Some(loaded);
                }
                computed += 1;
                embedder
                    .as_mut()
                    .unwrap()
                    .embed(&[&chunk.embed_text])?
                    .remove(0)
            };
            if !valid_vector(&vector) {
                return Err("некорректный вектор".into());
            }
            vectors.push(vector);
        }
        let refs = chunked
            .chunks
            .iter()
            .zip(&vectors)
            .map(|(c, v)| (c.embed_hash.as_str(), v.as_slice()))
            .collect::<Vec<_>>();
        index.replace_document(
            &DocumentRecord {
                path: &doc.path,
                module: doc.path.split('/').nth(1).unwrap_or("."),
                title: &chunked.title,
                size: doc.source.len() as u64,
                modified_ns: None,
                hash: &doc.hash,
                chunks: &chunked.chunks,
                text: None,
            },
            &refs,
        )?;
        eprintln!(
            "{} /5000, reused={reused}, computed={computed}",
            reused + computed
        );
    }
    if index.chunk_count()? != 5000 || index.vector_count()? != 5000 {
        return Err("неполный индекс".into());
    }
    save(
        &format!("{output}.complete.json"),
        &json!({"chunks":5000,"reused":reused,"computed":computed,"device":if cpu {"cpu"} else {"directml"},"index_blake3":digest(&fs::read(output)?),"corpus_blake3":digest(&serde_json::to_vec(corpus)?)}),
    )
}

struct Timed {
    inner: BuiltinEmbedder,
    seconds: f64,
    calls: usize,
}
impl Embedder for Timed {
    fn fingerprint(&self) -> &ModelFingerprint {
        self.inner.fingerprint()
    }
    fn embed(&mut self, texts: &[&str]) -> std::result::Result<Vec<Vec<f32>>, EmbedError> {
        self.inner.embed(texts)
    }
    fn embed_query(&mut self, text: &str) -> std::result::Result<Vec<f32>, EmbedError> {
        let start = Instant::now();
        let result = self.inner.embed_query(text);
        self.seconds = start.elapsed().as_secs_f64();
        self.calls += 1;
        result
    }
}
fn quantile(sorted: &[f64], p: f64) -> f64 {
    sorted[(sorted.len() as f64 * p).ceil() as usize - 1]
}
fn run(inputs: &Inputs, database: &str, models: &Path, output: &str) -> Result<()> {
    if inputs.queries.len() < 25 || inputs.queries.iter().any(|q| q.trim().is_empty()) {
        return Err("нужно не менее25 непустых вопросов".into());
    }
    let complete: serde_json::Value = read(&format!("{database}.complete.json"))?;
    if complete["index_blake3"] != digest(&fs::read(database)?) {
        return Err("индекс изменился после подготовки".into());
    }
    let index = Index::open_read_only(Path::new(database))?;
    if index.chunk_count()? != 5000 || index.vector_count()? != 5000 {
        return Err("неполный индекс".into());
    }
    let mut rows = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    let load = Instant::now();
    let inner = model(models, Accelerator::Cpu)?;
    if inner.device() != Device::Cpu {
        return Err("нужен CPU".into());
    }
    let load_seconds = load.elapsed().as_secs_f64();
    let execution = format!("{:?}", inner.execution());
    let mut timed = Timed {
        inner,
        seconds: 0.0,
        calls: 0,
    };
    let settings = SearchSettings::default();
    let request = |query: &str| SearchRequest {
        query: query.into(),
        ..Default::default()
    };
    let start = Instant::now();
    search(
        &index,
        &mut timed,
        None,
        &settings,
        &request(&inputs.queries[0]),
    )?;
    let first_seconds = start.elapsed().as_secs_f64();
    let mut times = Vec::new();
    let mut embeds = Vec::new();
    let mut baselines = HashMap::new();
    for round in 0..4 {
        for offset in 0..inputs.queries.len() {
            let i = (offset + round * 7) % inputs.queries.len();
            let query = &inputs.queries[i];
            let before_calls = timed.calls;
            let start = Instant::now();
            let response = search(&index, &mut timed, None, &settings, &request(query));
            let seconds = start.elapsed().as_secs_f64();
            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    writeln!(
                        rows,
                        "{}",
                        json!({"query":query,"round":round,"seconds":seconds,"error":error.to_string()})
                    )?;
                    rows.flush()?;
                    return Err(error.into());
                }
            };
            if response.applied.score_kind != ScoreKind::Cosine
                || response.rerank_error.is_some()
                || timed.calls != before_calls + 1
                || response.hits.is_empty()
                || response.hits.iter().any(|h| !h.score.is_finite())
            {
                return Err("неожиданный путь поиска или пустая выдача при пороге0".into());
            }
            let hits = response.hits.iter().map(|h| json!({"path":h.path,"heading":h.heading_chain,"line":h.first_line,"score":h.score})).collect::<Vec<_>>();
            let hash = digest(&serde_json::to_vec(&hits)?);
            let stable = baselines.entry(i).or_insert_with(|| hash.clone()) == &hash;
            writeln!(
                rows,
                "{}",
                json!({"query":query,"round":round,"seconds":seconds,"embed_seconds":timed.seconds,"stable":stable,"hits":hits})
            )?;
            rows.flush()?;
            if !stable {
                return Err("выдача изменилась между повторами".into());
            }
            times.push(seconds);
            embeds.push(timed.seconds);
        }
    }
    times.sort_by(f64::total_cmp);
    embeds.sort_by(f64::total_cmp);
    let summary = json!({"status":"complete-core-only","execution":execution,"chunks":5000,"unique_queries":inputs.queries.len(),"observations":times.len(),"load_seconds":load_seconds,"first_search_seconds":first_seconds,"median_seconds":quantile(&times,0.5),"p95_seconds":quantile(&times,0.95),"max_seconds":times.last(),"under_one_second":times.iter().filter(|t| **t < 1.0).count(),"embed_median_seconds":quantile(&embeds,0.5),"embed_p95_seconds":quantile(&embeds,0.95),"index_blake3":complete["index_blake3"],"inputs_blake3":digest(&serde_json::to_vec(&inputs.queries)?),"boundary":"core search, no IPC/UI, warm model, OS cache uncontrolled"});
    save(&format!("{output}.summary.json"), &summary)?;
    println!("{summary}");
    Ok(())
}
fn main() -> Result<()> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    match a.first().map(String::as_str) {
        Some("freeze") if a.len() == 3 => freeze(&read(&a[1])?, &a[2]),
        Some("validate") if a.len() == 2 => validate(&read(&a[1])?),
        Some("cache-plan") if a.len() == 4 => cache_plan(&read(&a[1])?, &read(&a[2])?, &a[3]),
        Some("preflight") if a.len() == 2 => guarded(&a[1], 8, || Ok(())),
        Some("prepare") if a.len() == 5 || (a.len() == 6 && a[5] == "cpu") => guarded(&a[3], 8, || prepare(&read(&a[1])?, &read(&a[2])?, &a[3], Path::new(&a[4]), a.len() == 6)),
        Some("run") if a.len() == 5 => guarded(&a[4], 6, || run(&read(&a[1])?, &a[2], Path::new(&a[3]), &a[4])),
        _ => Err("freeze <inputs.json> <new-corpus.json> | validate <corpus.json> | cache-plan <corpus.json> <inputs.json> <new-plan.json> | preflight <new-log-prefix> | prepare <corpus.json> <inputs.json> <new-index.sqlite> <models> [cpu] | run <inputs.json> <index.sqlite> <models> <new-rows.jsonl>".into()),
    }
}
