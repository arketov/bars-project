//! IDEA-06: воспроизводимый бенч реального search. Методика — search-quality-benchmark.md.
mod quality;
#[path = "quality/registry.rs"]
mod registry;

use bars_core::indexing::chunking::{ChunkingParams, chunk_document};
use bars_core::indexing::indexer::{IndexSettings, UpdateOutcome, update_index};
use bars_core::models::{
    Accelerator, BuiltinEmbedder, BuiltinReranker, EmbedError, Embedder, ModelFingerprint,
    RerankError, Reranker,
};
use bars_core::search::{SearchRequest, SearchSettings, candidate_trace, search};
use bars_core::storage::{DocumentRecord, Index, Journal};
use quality::{Row, digest, file_digest, load_cases, pack_hits, validate_cases};
use serde_json::json;
use std::collections::{BTreeSet, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::time::Instant;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    match a.first().map(String::as_str) {
        Some("prepare") if (4..=6).contains(&a.len()) => {
            let variant = a.get(4).map_or("A", String::as_str);
            let seed = a.get(5).filter(|s| *s != "-").map(Path::new);
            prepare(Path::new(&a[1]), Path::new(&a[2]), Path::new(&a[3]), variant, seed)
        }
        Some("validate") if a.len()==3 => {
            let cases=load_cases(Path::new(&a[2]))?;
            validate_cases(Path::new(&a[1]),&cases)?;
            eprintln!("валидно вопросов: {}",cases.len()); Ok(())
        }
        Some("run") if a.len()==10 || a.len()==11 => run(&a),
        Some("verify") if a.len()==3 => verify_vectors(Path::new(&a[1]), Path::new(&a[2])),
        _ => Err("prepare <source-project> <new-workspace> <models> [A|B|C|D] [seed-index|-] | validate <snapshot-project> <cases.json> | verify <workspace> <models> | run <workspace> <cases.json> <models> <output-dir> <split> <candidates-csv> <K-csv> <repeats> <auto|cpu> [all|gold|first|nochats]".into())
    }
}

fn copy_docs(
    source: &Path,
    target: &Path,
    relative: &Path,
    manifest: &mut Vec<serde_json::Value>,
) -> Result<()> {
    let mut entries = fs::read_dir(source.join(relative))?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(format!("ссылка в корпусе: {}", entry.path().display()).into());
        }
        let rel = relative.join(entry.file_name());
        if kind.is_dir() {
            copy_docs(source, target, &rel, manifest)?;
        } else if rel
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("md"))
        {
            let bytes = fs::read(entry.path())?;
            let dest = target.join(&rel);
            fs::create_dir_all(dest.parent().ok_or("нет родителя")?)?;
            fs::write(dest, &bytes)?;
            manifest.push(json!({"path":rel.to_string_lossy().replace('\\',"/"),"blake3":digest(&bytes),"bytes":bytes.len()}));
        }
    }
    Ok(())
}

/// `variant`: A — штатная нарезка, B/C — реестры по `registry.rs`, D — `max_chars=1500`
/// После 6.3.2 A уже включает продуктовый B, а B/C применяются поверх него.
/// Исторические A–D воспроизводятся ревизией замера 24-10-2026 и сохранёнными снимками.
/// (пункт 6.3.1). `seed` — индекс, векторы которого переиспользуются; `-` — без него.
fn prepare(
    source: &Path,
    workspace: &Path,
    models: &Path,
    variant: &str,
    seed: Option<&Path>,
) -> Result<()> {
    if workspace.exists() {
        return Err("prepare требует новую папку".into());
    }
    if !matches!(variant, "A" | "B" | "C" | "D") {
        return Err("вариант нарезки: A/B/C/D".into());
    }
    // Индекс строится штатной нарезкой, чтобы seed подошёл; вариант заменяет чанки после
    // и переиспользует векторы чанков с тем же текстом.
    let chunking = ChunkingParams::default();
    let small = ChunkingParams {
        max_chars: registry::WINDOW,
        ..ChunkingParams::default()
    };
    let snapshot = workspace.join("project");
    fs::create_dir_all(&snapshot)?;
    let mut files = Vec::new();
    copy_docs(source, &snapshot, Path::new("docs"), &mut files)?;
    let db = workspace.join("index.sqlite");
    let default_seed = source.join(".bars/index.sqlite");
    let seed = seed.unwrap_or(&default_seed);
    if seed.exists() {
        // VACUUM INTO учитывает WAL; исходный индекс открыт только для чтения.
        let conn = rusqlite::Connection::open_with_flags(
            seed,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        conn.execute("VACUUM INTO ?1", [db.to_string_lossy().as_ref()])?;
    }
    let started = Instant::now();
    let mut model = BuiltinEmbedder::bge_m3(models, Accelerator::Auto)?;
    let load = started.elapsed().as_secs_f64();
    let mut index = Index::open(&db, Journal::Delete)?;
    let settings = IndexSettings {
        chunking,
        docs_root: "docs".into(),
    };
    let mut outcome = update_index(&snapshot, &mut index, &settings, &mut model)?;
    if let UpdateOutcome::ReindexRequired(reason) = &outcome
        && seed.exists()
    {
        // Seed другой модели или нарезки: индекс строится заново, без чужих векторов.
        eprintln!("seed не подошёл ({reason:?}), индекс строится заново");
        drop(index);
        fs::remove_file(&db)?;
        index = Index::open(&db, Journal::Delete)?;
        outcome = update_index(&snapshot, &mut index, &settings, &mut model)?;
    }
    let UpdateOutcome::Updated(report) = outcome else {
        return Err(format!("несовместимый seed: {outcome:?}").into());
    };
    if !report.problems.is_empty() {
        return Err(format!("ошибки индекса: {:?}", report.problems).into());
    }
    let indexed = index.documents()?;
    let (mut rechunked, mut embedded) = (0, 0);
    if variant != "A" {
        for doc in &indexed {
            let text = fs::read_to_string(snapshot.join(&doc.path))?;
            let chunked = chunk_document(&doc.path, &text, &settings.chunking);
            let chunks = if variant == "D" {
                chunk_document(&doc.path, &text, &small).chunks
            } else {
                registry::rechunk(&doc.path, &chunked.chunks, variant)
            };
            if chunks == chunked.chunks {
                continue;
            }
            let mut missing: Vec<(&str, &str)> = Vec::new();
            for chunk in &chunks {
                if !index.has_vector(&chunk.embed_hash)?
                    && !missing.iter().any(|(hash, _)| *hash == chunk.embed_hash)
                {
                    missing.push((&chunk.embed_hash, &chunk.embed_text));
                }
            }
            let texts: Vec<&str> = missing.iter().map(|(_, text)| *text).collect();
            let vectors = if texts.is_empty() {
                Vec::new()
            } else {
                model.embed(&texts)?
            };
            let new_vectors: Vec<(&str, &[f32])> = missing
                .iter()
                .zip(&vectors)
                .map(|((hash, _), vector)| (*hash, vector.as_slice()))
                .collect();
            index.replace_document(
                &DocumentRecord {
                    path: &doc.path,
                    module: &module_of(&doc.path),
                    title: &chunked.title,
                    size: doc.size,
                    modified_ns: doc.modified_ns,
                    hash: &doc.hash,
                    chunks: &chunks,
                },
                &new_vectors,
            )?;
            rechunked += 1;
            embedded += missing.len();
        }
    }
    let expected = bars_core::indexing::walk::walk_docs(&snapshot, Path::new("docs"))?
        .files
        .len();
    if indexed.len() != expected {
        return Err("число документов индекса и снимка различается".into());
    }
    for doc in index.documents()? {
        if !files
            .iter()
            .any(|m| m["path"] == doc.path && m["blake3"] == doc.hash)
        {
            return Err(format!("не совпал индекс: {}", doc.path).into());
        }
    }
    let metadata = json!({"source":source,"files":files,"documents":index.document_count()?,"chunks":index.chunk_count()?,
        "vectors":index.vector_count()?,"embed":format!("{:?}",model.fingerprint()),"device":format!("{:?}",model.device()),
        "layout":format!("{:?}",index.stored_layout()?),"variant":variant,"registry_window":registry::WINDOW,"variant_d_max_chars":small.max_chars,
        "rechunked_documents":rechunked,"rechunk_embedded":embedded,"model_load_secs":load,"index_seconds":started.elapsed().as_secs_f64()-load,
        "update":format!("{report:?}"),"index_blake3":file_digest(&db)?});
    fs::write(
        workspace.join("manifest.json"),
        serde_json::to_vec_pretty(&metadata)?,
    )?;
    eprintln!(
        "{} документов, {} чанков; вариант {variant}: переделано {rechunked}, новых векторов {embedded}; {report:?}",
        index.document_count()?,
        index.chunk_count()?
    );
    Ok(())
}

struct TimedEmbed<'a> {
    inner: &'a mut BuiltinEmbedder,
    seconds: f64,
    vector: Vec<f32>,
}
impl Embedder for TimedEmbed<'_> {
    fn fingerprint(&self) -> &ModelFingerprint {
        self.inner.fingerprint()
    }
    fn embed(&mut self, texts: &[&str]) -> std::result::Result<Vec<Vec<f32>>, EmbedError> {
        self.inner.embed(texts)
    }
    fn embed_query(&mut self, text: &str) -> std::result::Result<Vec<f32>, EmbedError> {
        let started = Instant::now();
        let v = self.inner.embed_query(text)?;
        self.seconds = started.elapsed().as_secs_f64();
        self.vector = v.clone();
        Ok(v)
    }
}
struct TimedRerank<'a> {
    inner: &'a mut BuiltinReranker,
    seconds: f64,
}
impl Reranker for TimedRerank<'_> {
    fn rerank(&mut self, q: &str, docs: &[&str]) -> std::result::Result<Vec<f32>, RerankError> {
        let started = Instant::now();
        let result = self.inner.rerank(q, docs);
        self.seconds = started.elapsed().as_secs_f64();
        result
    }
}
fn numbers(s: &str) -> Result<Vec<usize>> {
    let v = s
        .split(',')
        .map(str::parse)
        .collect::<std::result::Result<Vec<usize>, _>>()?;
    if v.is_empty() || v.iter().any(|n| *n == 0 || *n > 100) {
        return Err("глубины: 1…100".into());
    }
    Ok(v)
}
fn append(f: &mut File, row: &Row) -> Result<()> {
    serde_json::to_writer(&mut *f, row)?;
    writeln!(f)?;
    f.flush()?;
    Ok(())
}

fn run(a: &[String]) -> Result<()> {
    let workspace = Path::new(&a[1]);
    let cases_path = Path::new(&a[2]);
    let models = Path::new(&a[3]);
    let out = Path::new(&a[4]);
    let split = &a[5];
    let counts = numbers(&a[6])?;
    let depths = numbers(&a[7])?;
    let repeats: usize = a[8].parse()?;
    if repeats == 0 {
        return Err("нужен хотя бы один повтор".into());
    }
    let accelerator = match a[9].as_str() {
        "cpu" => Accelerator::Cpu,
        "auto" => Accelerator::Auto,
        _ => return Err("устройство: auto/cpu".into()),
    };
    let scope = a.get(10).map_or("all", String::as_str);
    let all = load_cases(cases_path)?;
    validate_cases(&workspace.join("project"), &all)?;
    let db = workspace.join("index.sqlite");
    let index = Index::open(&db, Journal::Delete)?;
    let project_modules: BTreeSet<String> = index
        .documents()?
        .iter()
        .map(|d| module_of(&d.path))
        .collect();
    let mut cases = Vec::new();
    for case in all.iter().filter(|c| c.split == *split) {
        if let Some(modules) = scope_modules(scope, case, &project_modules)? {
            cases.push((case, modules));
        }
    }
    if cases.is_empty() {
        return Err("выборка пуста".into());
    }
    let manifest_bytes = fs::read(workspace.join("manifest.json"))?;
    let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes)?;
    if manifest["index_blake3"] != file_digest(&db)? {
        return Err("индекс изменён после prepare".into());
    }
    for f in manifest["files"].as_array().ok_or("нет files")? {
        let p = f["path"].as_str().ok_or("нет path")?;
        if f["blake3"] != file_digest(&workspace.join("project").join(p))? {
            return Err(format!("изменён снимок: {p}").into());
        }
    }
    fs::create_dir_all(out)?;
    let mut config = json!({"dataset":file_digest(cases_path)?,"manifest":digest(&manifest_bytes),"binary":file_digest(&std::env::current_exe()?)?,
        "split":split,"candidates":counts,"depths":depths,"repeats":repeats,"accelerator":a[9],"threshold":0,"models":model_manifest(models)?});
    // Без области в конфигурации остаются старые прогоны: их можно продолжить.
    if scope != "all" {
        config["scope"] = json!(scope);
    }
    let cfg = out.join("config.json");
    if cfg.exists() {
        let previous: serde_json::Value = serde_json::from_slice(&fs::read(&cfg)?)?;
        if previous != config {
            return Err("параметры/бинарник изменились: нужен новый output-dir".into());
        }
    } else {
        fs::write(cfg, serde_json::to_vec_pretty(&config)?)?;
    }
    let rows_path = out.join("rows.jsonl");
    let mut completed = HashSet::new();
    if rows_path.exists() {
        for line in BufReader::new(File::open(&rows_path)?).lines() {
            let row: Row = serde_json::from_str(&line?)?;
            if !completed.insert(row.key()) {
                return Err("дубли в rows.jsonl".into());
            }
        }
    }
    let mut output = OpenOptions::new()
        .create(true)
        .append(true)
        .open(rows_path)?;
    let started = Instant::now();
    let mut model = BuiltinEmbedder::bge_m3(models, accelerator)?;
    let embed_load = started.elapsed().as_secs_f64();
    let started = Instant::now();
    let mut rr = BuiltinReranker::bge_reranker_v2_m3(models, accelerator)?;
    let rerank_load = started.elapsed().as_secs_f64();
    let settings = SearchSettings {
        results: 200,
        results_reranked: 100,
        max_results: 200,
        max_rerank_candidates: 100,
        min_score_vector: 0.0,
        min_score_rerank: 0.0,
        ..Default::default()
    };
    let warm = SearchRequest {
        query: "порядок запуска ботов".into(),
        rerank_candidates: Some(*depths.iter().max().ok_or("нет K")?),
        ..Default::default()
    };
    let started = Instant::now();
    let response = search(&index, &mut model, Some(&mut rr), &settings, &warm)?;
    if let Some(error) = response.rerank_error {
        return Err(error.into());
    }
    let mut sessions = OpenOptions::new()
        .create(true)
        .append(true)
        .open(out.join("sessions.jsonl"))?;
    writeln!(
        sessions,
        "{}",
        json!({"embed_device":format!("{:?}",model.device()),"rerank_device":format!("{:?}",rr.device()),
        "embed_load":embed_load,"rerank_load":rerank_load,"warmup":started.elapsed().as_secs_f64(),"resumed":completed.len()})
    )?;
    for repeat in 0..repeats {
        for (ci, (case, modules)) in cases.iter().enumerate() {
            for &count in &counts {
                let request = SearchRequest {
                    query: case.query.clone(),
                    modules: modules.clone(),
                    candidates: Some(count),
                    ..Default::default()
                };
                let mut modes = vec![0];
                modes.extend(depths.iter().copied());
                if repeat % 2 == 1 {
                    modes.reverse();
                }
                for k in modes {
                    let search_done =
                        completed.contains(&format!("{}:{count}:{k}:{repeat}:search", case.id));
                    let trace_needed = k == 0
                        && repeat == 0
                        && ["fts", "vector", "fused"].iter().any(|mode| {
                            !completed.contains(&format!("{}:{count}:0:0:{mode}", case.id))
                        });
                    if search_done && !trace_needed {
                        continue;
                    }
                    let mut embed = TimedEmbed {
                        inner: &mut model,
                        seconds: 0.0,
                        vector: Vec::new(),
                    };
                    let mut rerank = TimedRerank {
                        inner: &mut rr,
                        seconds: 0.0,
                    };
                    let mut req = request.clone();
                    req.rerank_candidates = Some(k.max(1));
                    let started = Instant::now();
                    let response = search(
                        &index,
                        &mut embed,
                        if k == 0 { None } else { Some(&mut rerank) },
                        &settings,
                        &req,
                    )?;
                    let total = started.elapsed().as_secs_f64();
                    if let Some(error) = response.rerank_error {
                        return Err(format!("{} K={k}: {error}", case.id).into());
                    }
                    let row = Row {
                        id: case.id.clone(),
                        candidates: count,
                        k,
                        repeat,
                        mode: "search".into(),
                        total,
                        embed: embed.seconds,
                        rerank: rerank.seconds,
                        hits: pack_hits(&response.hits, case),
                    };
                    if !search_done {
                        append(&mut output, &row)?;
                    }
                    if trace_needed {
                        let lists = candidate_trace(&index, &embed.vector, &req, count)?;
                        if lists[2]
                            .iter()
                            .map(|h| h.chunk_id)
                            .ne(response.hits.iter().map(|h| h.chunk_id))
                        {
                            return Err("trace не совпадает с search".into());
                        }
                        for (mode, hits) in ["fts", "vector", "fused"].into_iter().zip(lists) {
                            if completed.contains(&format!("{}:{count}:0:0:{mode}", case.id)) {
                                continue;
                            }
                            append(
                                &mut output,
                                &Row {
                                    mode: mode.into(),
                                    hits: pack_hits(&hits, case),
                                    ..row.clone()
                                },
                            )?;
                        }
                    }
                    eprintln!(
                        "{} {}/{} {} C={count} K={k} repeat={} {:.3}s",
                        split,
                        ci + 1,
                        cases.len(),
                        case.id,
                        repeat + 1,
                        total
                    );
                }
            }
        }
    }
    Ok(())
}
/// Модуль документа по пути от корня проекта (Т-7): первая папка под `docs/`, файл в корне — `.`.
fn module_of(path: &str) -> String {
    let inner = path.strip_prefix("docs/").unwrap_or(path);
    match inner.split_once('/') {
        Some((module, _)) => module.to_string(),
        None => ".".to_string(),
    }
}

/// Область поиска вопроса по варианту замера module-scope.md; `None` — вопрос в варианте не участвует.
fn scope_modules(
    scope: &str,
    case: &quality::Case,
    project: &BTreeSet<String>,
) -> Result<Option<Vec<String>>> {
    let gold: BTreeSet<String> = case.gold.iter().map(|g| module_of(&g.path)).collect();
    Ok(match scope {
        "all" => Some(Vec::new()),
        "gold" => (!gold.is_empty()).then(|| gold.into_iter().collect()),
        "first" => case.gold.first().map(|g| vec![module_of(&g.path)]),
        "nochats" => (!gold.is_empty() && !gold.contains("chats"))
            .then(|| project.iter().filter(|m| *m != "chats").cloned().collect()),
        _ => return Err("область: all/gold/first/nochats".into()),
    })
}

fn verify_vectors(workspace: &Path, models: &Path) -> Result<()> {
    use bars_core::indexing::chunking::chunk_document;
    let index = Index::open(&workspace.join("index.sqlite"), Journal::Delete)?;
    let layout = index.stored_layout()?.ok_or("нет layout")?;
    let mut model = BuiltinEmbedder::bge_m3(models, Accelerator::Auto)?;
    let mut stmt = index.conn().prepare("SELECT d.path, c.embed_hash FROM chunks c JOIN documents d ON d.id=c.document_id ORDER BY d.path,c.id")?;
    let records = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if records.len() < 3 {
        return Err("недостаточно чанков для проверки".into());
    }
    let mut checks = Vec::new();
    for position in [0, records.len() / 2, records.len() - 1] {
        let (path, hash) = &records[position];
        let source = fs::read_to_string(workspace.join("project").join(path))?;
        let chunked = chunk_document(path, &source, &layout.chunking);
        let chunk = chunked
            .chunks
            .iter()
            .find(|c| &c.embed_hash == hash)
            .ok_or("чанк не соответствует снимку")?;
        let stored = index.vector(hash)?.ok_or("нет вектора")?;
        let mut vectors = model.embed(&[&chunk.embed_text])?;
        let fresh = vectors.pop().ok_or("модель не вернула вектор")?;
        if fresh.len() != stored.len() {
            return Err("размерности векторов различаются".into());
        }
        let norm = fresh.iter().map(|v| v * v).sum::<f32>().sqrt();
        let fresh: Vec<_> = fresh.into_iter().map(|v| v / norm).collect();
        let cosine: f32 = fresh.iter().zip(&stored).map(|(a, b)| a * b).sum();
        let max_abs = fresh
            .iter()
            .zip(&stored)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        if !cosine.is_finite() || cosine < 0.999 || max_abs > 0.005 {
            return Err(format!(
                "свежий вектор не совпал: {path}, cosine={cosine}, max_abs={max_abs}"
            )
            .into());
        }
        checks.push(json!({"path":path,"embed_hash":hash,"cosine":cosine,"max_abs":max_abs}));
    }
    #[cfg(windows)]
    let onnx_runtime = ort::info();
    #[cfg(not(windows))]
    let onnx_runtime = "см. lockfile fastembed/ort";
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"device":format!("{:?}",model.device()),"onnx_runtime":onnx_runtime,"checked":checks,"passed":true})
        )?
    );
    Ok(())
}

fn model_manifest(root: &Path) -> Result<Vec<serde_json::Value>> {
    fn visit(root: &Path, path: &Path, out: &mut Vec<serde_json::Value>) -> Result<()> {
        let mut entries = fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            if e.file_type()?.is_dir() {
                visit(root, &p, out)?;
            } else if !p.components().any(|c| c.as_os_str() == ".locks") {
                out.push(json!({"path":p.strip_prefix(root)?,"bytes":p.metadata()?.len(),"blake3":file_digest(&p)?}));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    for name in ["models--BAAI--bge-m3", "models--rozgo--bge-reranker-v2-m3"] {
        visit(root, &root.join(name), &mut files)?;
    }
    Ok(files)
}
