//! Изолированные опыты этапа 11: docs/chats/2026-10-18-inference-design/stage11-plan.md.
#![allow(dead_code)]
#[path = "../bench_memory.rs"]
mod native_memory;
#[path = "../quality/mod.rs"]
mod quality;

use bars_core::models::{
    Accelerator, BuiltinEmbedder, EmbedError, Embedder, ModelFingerprint, RerankError, Reranker,
    bge_m3_fingerprint, sigmoid,
};
use bars_core::search::{SearchRequest, SearchSettings, search};
use bars_core::storage::{Index, Journal};
use fastembed::{
    EmbeddingModel, RerankInitOptions, RerankerModel, TextEmbedding, TextInitOptions, TextRerank,
};
use quality::{Hit, file_digest, load_cases, pack_hits, validate_cases};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Serialize, Deserialize)]
struct Input {
    id: String,
    query: String,
    split: String,
    gold_count: usize,
    hits: Vec<Hit>,
    documents: Vec<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub name: String,
    device: String,
    threads: Option<usize>,
    batch: usize,
    sorted: bool,
    max_length: usize,
    repeats: usize,
    #[serde(default)]
    ids: Vec<String>,
    #[serde(default)]
    spinning: Option<bool>,
    #[serde(default)]
    model_path: Option<PathBuf>,
    #[serde(default)]
    tokenizer_dir: Option<PathBuf>,
    #[serde(default)]
    fixed_padding: bool,
    #[serde(default)]
    embedding_dynamic: bool,
}
impl Config {
    fn validate(&self) -> Result<()> {
        if !["cpu", "directml"].contains(&self.device.as_str())
            || self.batch == 0
            || self.batch > 64
            || self.max_length < 16
            || self.max_length > 8192
            || self.repeats == 0
            || self.threads == Some(0)
        {
            return Err("invalid experiment configuration".into());
        }
        Ok(())
    }
}

fn providers(device: &str) -> Vec<fastembed::ExecutionProviderDispatch> {
    if device == "directml" {
        vec![
            ort::ep::DirectML::default()
                .with_device_id(0)
                .build()
                .error_on_failure(),
        ]
    } else {
        Vec::new()
    }
}

pub struct ExperimentalReranker {
    model: TextRerank,
    cfg: Config,
    tokens: usize,
    padded: usize,
    logits: Vec<f32>,
}
impl ExperimentalReranker {
    pub fn load(models: &Path, cfg: Config) -> Result<Self> {
        cfg.validate()?;
        let mut opts = RerankInitOptions::new(RerankerModel::BGERerankerV2M3)
            .with_cache_dir(models.into())
            .with_max_length(cfg.max_length)
            .with_execution_providers(providers(&cfg.device));
        if let Some(n) = cfg.threads {
            opts = opts.with_intra_threads(n);
        }
        if let Some(spin) = cfg.spinning {
            opts = opts.with_session_config(
                "session.intra_op.allow_spinning",
                if spin { "1" } else { "0" },
            );
        }
        let mut model = if let Some(path) = &cfg.model_path {
            let dir = cfg
                .tokenizer_dir
                .as_ref()
                .ok_or("custom model needs tokenizer_dir")?;
            let files = fastembed::TokenizerFiles {
                tokenizer_file: fs::read(dir.join("tokenizer.json"))?,
                config_file: fs::read(dir.join("config.json"))?,
                special_tokens_map_file: fs::read(dir.join("special_tokens_map.json"))?,
                tokenizer_config_file: fs::read(dir.join("tokenizer_config.json"))?,
            };
            TextRerank::try_new_from_user_defined(
                fastembed::UserDefinedRerankingModel::new(path.clone(), files),
                opts.into(),
            )?
        } else {
            TextRerank::try_new(opts)?
        };
        if cfg.fixed_padding {
            let mut padding = serde_json::to_value(
                model
                    .tokenizer
                    .get_padding()
                    .ok_or("missing padding params")?,
            )?;
            padding["strategy"] = json!({"Fixed":cfg.max_length});
            model
                .tokenizer
                .with_padding(Some(serde_json::from_value(padding)?));
        }
        Ok(Self {
            model,
            cfg,
            tokens: 0,
            padded: 0,
            logits: Vec::new(),
        })
    }
}

fn order_for_lengths(lengths: &[usize], sorted: bool) -> Vec<usize> {
    let mut order: Vec<_> = (0..lengths.len()).collect();
    if sorted {
        order.sort_by_key(|&i| lengths[i]);
    }
    order
}
fn restore(values: &[(usize, f32)], order: &[usize]) -> Result<Vec<f32>> {
    let mut scores = vec![f32::NAN; order.len()];
    for &(index, value) in values {
        let target = *order.get(index).ok_or("model index out of range")?;
        if !scores[target].is_nan() || !value.is_finite() {
            return Err("duplicate index or nonfinite score".into());
        }
        scores[target] = value;
    }
    if scores.iter().any(|x| !x.is_finite()) {
        return Err("missing model score".into());
    }
    Ok(scores)
}
impl Reranker for ExperimentalReranker {
    fn rerank(&mut self, q: &str, docs: &[&str]) -> std::result::Result<Vec<f32>, RerankError> {
        let result = (|| -> Result<Vec<f32>> {
            let mut tokenizer = self.model.tokenizer.clone();
            tokenizer.with_padding(None);
            let lengths: Vec<_> = docs
                .iter()
                .map(|d| {
                    tokenizer
                        .encode((q, *d), true)
                        .map(|e| e.len())
                        .map_err(|e| e.to_string())
                })
                .collect::<std::result::Result<_, _>>()?;
            let order = order_for_lengths(&lengths, self.cfg.sorted);
            self.tokens = lengths.iter().sum();
            self.padded = order
                .chunks(self.cfg.batch)
                .map(|b| b.iter().map(|&i| lengths[i]).max().unwrap_or(0) * b.len())
                .sum();
            if self.cfg.fixed_padding {
                self.padded = self.cfg.max_length * docs.len();
            }
            let sorted: Vec<_> = order.iter().map(|&i| docs[i]).collect();
            let results = self.model.rerank(q, sorted, false, Some(self.cfg.batch))?;
            let indexed: Vec<_> = results.iter().map(|r| (r.index, r.score)).collect();
            self.logits = restore(&indexed, &order)?;
            Ok(self.logits.iter().copied().map(sigmoid).collect())
        })();
        result.map_err(|e| RerankError::Rerank(e.to_string()))
    }
}

fn emit(file: &mut File, value: impl Serialize) -> Result<()> {
    serde_json::to_writer(&mut *file, &value)?;
    writeln!(file)?;
    file.flush()?;
    Ok(())
}
fn verify_snapshot(workspace: &Path) -> Result<()> {
    let m: serde_json::Value = serde_json::from_slice(&fs::read(workspace.join("manifest.json"))?)?;
    if m["index_blake3"] != file_digest(&workspace.join("index.sqlite"))? {
        return Err("snapshot index changed".into());
    }
    for f in m["files"].as_array().ok_or("missing file manifest")? {
        let p = f["path"].as_str().ok_or("missing path")?;
        if f["blake3"] != file_digest(&workspace.join("project").join(p))? {
            return Err(format!("snapshot document changed: {p}").into());
        }
    }
    Ok(())
}
fn prepare(workspace: &Path, cases_path: &Path, models: &Path, out: &Path) -> Result<()> {
    verify_snapshot(workspace)?;
    let cases = load_cases(cases_path)?;
    validate_cases(&workspace.join("project"), &cases)?;
    if out.exists() {
        return Err("prepare requires new directory".into());
    }
    fs::create_dir_all(out)?;
    fs::copy(workspace.join("index.sqlite"), out.join("index.sqlite"))?;
    let index = Index::open(&out.join("index.sqlite"), Journal::Delete)?;
    let mut embed = BuiltinEmbedder::bge_m3(models, Accelerator::Auto)?;
    let settings = SearchSettings {
        results: 100,
        max_results: 100,
        min_score_vector: 0.,
        ..Default::default()
    };
    let mut inputs = Vec::new();
    for c in &cases {
        let request = SearchRequest {
            query: c.query.clone(),
            ..Default::default()
        };
        let response = search(&index, &mut embed, None, &settings, &request)?;
        let hits: Vec<_> = response.hits.into_iter().take(15).collect();
        let docs = hits
            .iter()
            .map(|h| format!("{}\n{}\n{}", h.path, h.heading_chain, h.text))
            .collect();
        inputs.push(Input {
            id: c.id.clone(),
            query: c.query.clone(),
            split: c.split.clone(),
            gold_count: c.gold.len(),
            hits: pack_hits(&hits, c),
            documents: docs,
        });
    }
    fs::write(out.join("inputs.json"), serde_json::to_vec_pretty(&inputs)?)?;
    fs::copy(cases_path, out.join("cases.json"))?;
    fs::copy(
        workspace.join("manifest.json"),
        out.join("source-manifest.json"),
    )?;
    fs::write(
        out.join("manifest.json"),
        serde_json::to_vec_pretty(
            &json!({"source":workspace,"cases_blake3":file_digest(cases_path)?,"inputs_blake3":file_digest(&out.join("inputs.json"))?,"index_blake3":file_digest(&out.join("index.sqlite"))?,"count":inputs.len()}),
        )?,
    )?;
    Ok(())
}

fn run(data: &Path, models: &Path, config: &Path, out: &Path) -> Result<()> {
    let cfg: Config = serde_json::from_slice(&fs::read(config)?)?;
    cfg.validate()?;
    if out.exists() {
        return Err("run requires new directory".into());
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(data.join("manifest.json"))?)?;
    if manifest["inputs_blake3"] != file_digest(&data.join("inputs.json"))? {
        return Err("inputs changed".into());
    }
    let all: Vec<Input> = serde_json::from_slice(&fs::read(data.join("inputs.json"))?)?;
    let inputs: Vec<_> = all
        .iter()
        .filter(|c| cfg.ids.is_empty() || cfg.ids.contains(&c.id))
        .collect();
    if inputs.is_empty() || (!cfg.ids.is_empty() && inputs.len() != cfg.ids.len()) {
        return Err("unknown/duplicate/empty selection".into());
    }
    fs::create_dir_all(out)?;
    fs::write(
        out.join("config.json"),
        serde_json::to_vec_pretty(
            &json!({"config":cfg,"manifest":manifest,"binary_blake3":file_digest(&std::env::current_exe()?)?,"ort":ort::info()}),
        )?,
    )?;
    let started = Instant::now();
    let mut model = ExperimentalReranker::load(models, cfg.clone())?;
    let load = started.elapsed().as_secs_f64();
    let first = inputs[0];
    let refs: Vec<_> = first.documents.iter().map(String::as_str).collect();
    let started = Instant::now();
    model.rerank(&first.query, &refs)?;
    let first_run = started.elapsed().as_secs_f64();
    fs::write(
        out.join("session.json"),
        serde_json::to_vec_pretty(
            &json!({"load":load,"first_run":first_run,"device":cfg.device,"registration_failure_fatal":cfg.device=="directml"}),
        )?,
    )?;
    let mut rows = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(out.join("rows.jsonl"))?;
    for repeat in 0..cfg.repeats {
        for i in 0..inputs.len() {
            let c = inputs[if repeat % 2 == 0 {
                i
            } else {
                inputs.len() - 1 - i
            }];
            let refs: Vec<_> = c.documents.iter().map(String::as_str).collect();
            let started = Instant::now();
            let scores = model.rerank(&c.query, &refs)?;
            let seconds = started.elapsed().as_secs_f64();
            let mut hits = c.hits.clone();
            for (hit, score) in hits.iter_mut().zip(&scores) {
                hit.score = *score;
            }
            hits.sort_by(|a, b| b.score.total_cmp(&a.score));
            emit(
                &mut rows,
                json!({"id":c.id,"repeat":repeat,"seconds":seconds,"tokens":model.tokens,"padded":model.padded,"logits":model.logits,"hits":hits}),
            )?;
            eprintln!(
                "{} {} {}/{} repeat={} {:.3}s",
                cfg.name,
                c.id,
                i + 1,
                inputs.len(),
                repeat,
                seconds
            );
        }
    }
    fs::write(
        out.join("complete.json"),
        serde_json::to_vec_pretty(
            &json!({"observations":inputs.len()*cfg.repeats,"rows_blake3":file_digest(&out.join("rows.jsonl"))?}),
        )?,
    )?;
    Ok(())
}

struct ExperimentalEmbedder {
    model: TextEmbedding,
    cfg: Config,
    seconds: f64,
    fingerprint: ModelFingerprint,
}
impl ExperimentalEmbedder {
    fn load(models: &Path, cfg: Config) -> Result<Self> {
        cfg.validate()?;
        if cfg.embedding_dynamic && (cfg.model_path.is_none() || cfg.batch != 1) {
            return Err(
                "dynamic embed experiment requires custom weights and single-text batches".into(),
            );
        }
        let mut opts = TextInitOptions::new(EmbeddingModel::BGEM3)
            .with_cache_dir(models.into())
            .with_max_length(8192)
            .with_execution_providers(providers(&cfg.device));
        if let Some(n) = cfg.threads {
            opts = opts.with_intra_threads(n);
        }
        let mut fingerprint = bge_m3_fingerprint();
        let model = if let Some(path) = &cfg.model_path {
            let dir = cfg
                .tokenizer_dir
                .as_ref()
                .ok_or("custom embed needs tokenizer_dir")?;
            let files = fastembed::TokenizerFiles {
                tokenizer_file: fs::read(dir.join("tokenizer.json"))?,
                config_file: fs::read(dir.join("config.json"))?,
                special_tokens_map_file: fs::read(dir.join("special_tokens_map.json"))?,
                tokenizer_config_file: fs::read(dir.join("tokenizer_config.json"))?,
            };
            let mut custom = fastembed::UserDefinedEmbeddingModel::new(fs::read(path)?, files)
                .with_pooling(fastembed::Pooling::Cls);
            custom.output_key = Some(fastembed::OutputKey::ByName("last_hidden_state"));
            if cfg.embedding_dynamic {
                custom = custom.with_quantization(fastembed::QuantizationMode::Dynamic);
            }
            fingerprint.source = "inference-experiment".into();
            fingerprint.model = format!("BAAI/bge-m3@{}", file_digest(path)?);
            TextEmbedding::try_new_from_user_defined(custom, opts.into())?
        } else {
            TextEmbedding::try_new(opts)?
        };
        Ok(Self {
            model,
            cfg,
            seconds: 0.,
            fingerprint,
        })
    }
}
impl Embedder for ExperimentalEmbedder {
    fn fingerprint(&self) -> &ModelFingerprint {
        &self.fingerprint
    }
    fn embed(&mut self, texts: &[&str]) -> std::result::Result<Vec<Vec<f32>>, EmbedError> {
        if texts.is_empty() {
            self.seconds = 0.;
            return Ok(Vec::new());
        }
        let start = Instant::now();
        let order = order_for_lengths(
            &texts.iter().map(|s| s.len()).collect::<Vec<_>>(),
            self.cfg.sorted,
        );
        let refs: Vec<_> = order.iter().map(|&i| texts[i]).collect();
        let vectors = if self.cfg.embedding_dynamic {
            let mut vectors = Vec::new();
            for text in refs {
                vectors.extend(
                    self.model
                        .embed([text], None)
                        .map_err(|e| EmbedError::Embed(e.to_string()))?,
                );
            }
            vectors
        } else {
            self.model
                .embed(refs, Some(self.cfg.batch))
                .map_err(|e| EmbedError::Embed(e.to_string()))?
        };
        if vectors.iter().any(|v| {
            v.len() != 1024
                || v.iter().any(|x| !x.is_finite())
                || (v.iter().map(|x| x * x).sum::<f32>() - 1.).abs() > 0.01
        }) {
            return Err(EmbedError::Embed(
                "invalid/unnormalized BGE dense vector".into(),
            ));
        }
        if vectors.len() != texts.len() {
            return Err(EmbedError::Count {
                expected: texts.len(),
                got: vectors.len(),
            });
        }
        let mut result = vec![Vec::new(); texts.len()];
        for (v, &i) in vectors.into_iter().zip(&order) {
            result[i] = v;
        }
        self.seconds = start.elapsed().as_secs_f64();
        Ok(result)
    }
}
struct TimedReranker {
    inner: Box<dyn Reranker>,
    seconds: f64,
}

struct SavedQueries {
    fingerprint: ModelFingerprint,
    vectors: std::collections::HashMap<String, Vec<f32>>,
}
impl Embedder for SavedQueries {
    fn fingerprint(&self) -> &ModelFingerprint {
        &self.fingerprint
    }
    fn embed(&mut self, texts: &[&str]) -> std::result::Result<Vec<Vec<f32>>, EmbedError> {
        texts
            .iter()
            .map(|text| {
                self.vectors
                    .get(*text)
                    .cloned()
                    .ok_or_else(|| EmbedError::Embed("запрос отсутствует в quality-кэше".into()))
            })
            .collect()
    }
}
struct TimedEmbedder {
    inner: Box<dyn Embedder>,
    seconds: f64,
}
impl Embedder for TimedEmbedder {
    fn fingerprint(&self) -> &ModelFingerprint {
        self.inner.fingerprint()
    }
    fn embed(&mut self, texts: &[&str]) -> std::result::Result<Vec<Vec<f32>>, EmbedError> {
        let start = Instant::now();
        let result = self.inner.embed(texts);
        self.seconds = start.elapsed().as_secs_f64();
        result
    }
    fn embed_query(&mut self, text: &str) -> std::result::Result<Vec<f32>, EmbedError> {
        let start = Instant::now();
        let result = self.inner.embed_query(text);
        self.seconds = start.elapsed().as_secs_f64();
        result
    }
}
impl Reranker for TimedReranker {
    fn rerank(&mut self, q: &str, docs: &[&str]) -> std::result::Result<Vec<f32>, RerankError> {
        let start = Instant::now();
        let result = self.inner.rerank(q, docs);
        self.seconds = start.elapsed().as_secs_f64();
        result
    }
}

fn load_embedding_profile(
    models: &Path,
    cfg: Config,
    profile: Option<&str>,
) -> Result<Box<dyn Embedder>> {
    match profile {
        Some("fp16") if cfg.device == "directml" => Ok(Box::new(BuiltinEmbedder::bge_m3_fp16(
            models,
            Accelerator::Auto,
        )?)),
        Some("int8") if cfg.device == "cpu" => Ok(Box::new(BuiltinEmbedder::bge_m3_int8(models)?)),
        Some(_) => Err("unknown builtin embedding profile or incompatible device".into()),
        None => Ok(Box::new(ExperimentalEmbedder::load(models, cfg)?)),
    }
}

fn full(data: &Path, models: &Path, config: &Path, out: &Path, staged_quality: bool) -> Result<()> {
    let cfg: Config = serde_json::from_slice(&fs::read(config)?)?;
    cfg.validate()?;
    if out.exists() {
        return Err("full requires new directory".into());
    }
    fs::create_dir_all(out)?;
    let inputs: Vec<Input> = serde_json::from_slice(&fs::read(data.join("inputs.json"))?)?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(data.join("manifest.json"))?)?;
    if manifest["inputs_blake3"] != file_digest(&data.join("inputs.json"))?
        || manifest["cases_blake3"] != file_digest(&data.join("cases.json"))?
    {
        return Err("full inputs/cases changed".into());
    }
    let cases = load_cases(&data.join("cases.json"))?;
    let embed_cfg = if manifest.get("embedding_config").is_some() {
        serde_json::from_value(manifest["embedding_config"].clone())?
    } else {
        Config {
            name: "production-embed".into(),
            device: "directml".into(),
            threads: None,
            batch: 4,
            sorted: true,
            max_length: 8192,
            repeats: 1,
            ids: Vec::new(),
            spinning: None,
            model_path: None,
            tokenizer_dir: None,
            fixed_padding: false,
            embedding_dynamic: false,
        }
    };
    let builtin_profile = manifest
        .get("builtin_embedding_profile")
        .and_then(|v| v.as_str());
    let int8 = builtin_profile == Some("int8")
        || (builtin_profile.is_none() && embed_cfg.embedding_dynamic);
    let memory_log = if staged_quality {
        let log = native_memory::windows::Log::create(&out.join("native-memory.jsonl"))?;
        log.reserve(if int8 { 4 } else { 6 })?;
        log.emit("config", "quality", json!({"staged":true,"dynamic_int8":int8,"builtin_profile":builtin_profile,"startup_commit_gib":if int8 {4}else{6}}));
        Some(log)
    } else {
        None
    };
    let memory_monitor = memory_log
        .as_ref()
        .map(|log| native_memory::windows::Monitor::start(log.clone()));
    let load = Instant::now();
    let mut embed = load_embedding_profile(models, embed_cfg, builtin_profile)?;
    let embed_execution = embed.execution();
    // Quality-only: don't retain two model sessions just to judge saved query vectors.
    let embed: Box<dyn Embedder> = if staged_quality {
        let fingerprint = embed.fingerprint().clone();
        let mut vectors = std::collections::HashMap::new();
        let mut saved = Vec::new();
        for input in inputs
            .iter()
            .filter(|c| cfg.ids.is_empty() || cfg.ids.contains(&c.id))
        {
            let vector = embed.embed_query(&input.query)?;
            saved.push(json!({"id":input.id,"vector":vector}));
            vectors.insert(input.query.clone(), vector);
        }
        fs::write(out.join("query-vectors.json"), serde_json::to_vec(&saved)?)?;
        drop(embed);
        Box::new(SavedQueries {
            fingerprint,
            vectors,
        })
    } else {
        embed
    };
    if let Some(log) = &memory_log {
        log.emit("marker", "queries_saved_embed_dropped", json!(null));
        log.reserve(4)?;
    }
    let mut embed = TimedEmbedder {
        inner: embed,
        seconds: 0.,
    };
    let (rr, rr_device): (Box<dyn Reranker>, String) = if cfg.name == "production" {
        let model =
            bars_core::models::BuiltinReranker::bge_reranker_v2_m3(models, Accelerator::Auto)?;
        if model.device() != bars_core::models::Device::DirectMl {
            return Err("unexpected CPU fallback in GPU quality comparison".into());
        }
        let device = format!("{:?}", model.device());
        (Box::new(model), device)
    } else {
        (
            Box::new(ExperimentalReranker::load(models, cfg.clone())?),
            cfg.device.clone(),
        )
    };
    let index = Index::open(&data.join("index.sqlite"), Journal::Delete)?;
    let mut timed_rr = TimedReranker {
        inner: rr,
        seconds: 0.,
    };
    let settings = SearchSettings {
        results: 100,
        results_reranked: 100,
        max_results: 100,
        min_score_rerank: 0.,
        ..Default::default()
    };
    let load = load.elapsed().as_secs_f64();
    let first = inputs
        .iter()
        .find(|c| cfg.ids.is_empty() || cfg.ids.contains(&c.id))
        .ok_or("empty full selection")?;
    let warm = Instant::now();
    let warm_result = search(
        &index,
        &mut embed,
        Some(&mut timed_rr),
        &settings,
        &SearchRequest {
            query: first.query.clone(),
            ..Default::default()
        },
    )?;
    if warm_result.rerank_error.is_some() {
        return Err("full warmup rerank fallback".into());
    }
    fs::write(
        out.join("session.json"),
        serde_json::to_vec_pretty(
            &json!({"load":load,"first_run":warm.elapsed().as_secs_f64(),"rerank_device":rr_device,"staged_quality":staged_quality,"builtin_embedding_profile":builtin_profile,
                "embedding_runtime":{"actual":embed_execution.actual,"precision":embed_execution.precision,"revision":embed_execution.revision,"fallback_reason":embed_execution.fallback_reason},
                "inputs_blake3":file_digest(&data.join("inputs.json"))?,"binary_blake3":file_digest(&std::env::current_exe()?)?}),
        )?,
    )?;
    let mut rows = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(out.join("rows.jsonl"))?;
    fs::write(out.join("config.json"), serde_json::to_vec_pretty(&cfg)?)?;
    for repeat in 0..cfg.repeats {
        for c in inputs
            .iter()
            .filter(|c| cfg.ids.is_empty() || cfg.ids.contains(&c.id))
        {
            let req = SearchRequest {
                query: c.query.clone(),
                ..Default::default()
            };
            let start = Instant::now();
            timed_rr.seconds = 0.;
            let result = search(&index, &mut embed, Some(&mut timed_rr), &settings, &req)?;
            let total = start.elapsed().as_secs_f64();
            if result.rerank_error.is_some() {
                return Err("full search rerank fallback".into());
            }
            let case = cases
                .iter()
                .find(|case| case.id == c.id)
                .ok_or("missing frozen case")?;
            let hits = pack_hits(&result.hits, case);
            let candidate_delta = hits
                .iter()
                .filter(|h| {
                    !c.hits.iter().any(|old| {
                        old.path == h.path
                            && old.heading == h.heading
                            && old.first == h.first
                            && old.last == h.last
                    })
                })
                .count();
            emit(
                &mut rows,
                json!({"id":c.id,"repeat":repeat,"seconds":total,"embed":embed.seconds,"rerank":timed_rr.seconds,"other":total-embed.seconds-timed_rr.seconds,"candidate_delta":candidate_delta,"hits":hits}),
            )?;
            eprintln!("full {} {} repeat={} {:.3}s", cfg.name, c.id, repeat, total);
        }
    }
    fs::write(
        out.join("complete.json"),
        serde_json::to_vec_pretty(&json!({"rows_blake3":file_digest(&out.join("rows.jsonl"))?}))?,
    )?;
    drop(timed_rr);
    drop(embed);
    if let Some(monitor) = memory_monitor {
        monitor.stop()?;
    }
    if let Some(log) = memory_log {
        log.emit("end", "quality_end", json!({"ok":true}));
    }
    Ok(())
}

fn embed_bench(data: &Path, models: &Path, config: &Path, out: &Path) -> Result<()> {
    let cfg: Config = serde_json::from_slice(&fs::read(config)?)?;
    cfg.validate()?;
    if out.exists() {
        return Err("embed requires new directory".into());
    }
    fs::create_dir_all(out)?;
    let inputs: Vec<Input> = serde_json::from_slice(&fs::read(data.join("inputs.json"))?)?;
    let load = Instant::now();
    let mut model = ExperimentalEmbedder::load(models, cfg.clone())?;
    let load = load.elapsed().as_secs_f64();
    model.embed_query("прогрев встроенной модели")?;
    let mut rows = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(out.join("rows.jsonl"))?;
    for repeat in 0..cfg.repeats {
        for c in inputs
            .iter()
            .filter(|c| cfg.ids.is_empty() || cfg.ids.contains(&c.id))
        {
            let t = Instant::now();
            let v = model.embed_query(&c.query)?;
            if v.iter().any(|x| !x.is_finite()) {
                return Err("nonfinite embed vector".into());
            }
            emit(
                &mut rows,
                json!({"id":c.id,"repeat":repeat,"seconds":t.elapsed().as_secs_f64(),"vector":v}),
            )?;
        }
    }
    // Fixed documents from the same frozen retrieval pool; throughput is a model microbenchmark.
    let mut texts: Vec<_> = inputs.iter().flat_map(|c| c.documents.iter()).collect();
    texts.sort();
    texts.dedup();
    texts.sort_by_key(|s| s.len());
    if texts.len() < 2 {
        return Err("embed document benchmark needs at least two distinct texts".into());
    }
    let mut selected: Vec<_> = (0..64.min(texts.len()))
        .map(|i| texts[i * (texts.len() - 1) / 63.min(texts.len() - 1)].as_str())
        .collect();
    // Freeze a mixed order independently of timing; sorted input would hide padding cost.
    selected.sort_by_cached_key(|s| quality::digest(s.as_bytes()));
    let mut indexing = Vec::new();
    for repeat in 0..cfg.repeats {
        let t = Instant::now();
        let vectors = model.embed(&selected)?;
        let seconds = t.elapsed().as_secs_f64();
        fs::write(
            out.join(format!("document-vectors-{repeat}.json")),
            serde_json::to_vec(&vectors)?,
        )?;
        indexing.push(json!({"repeat":repeat,"texts":selected.len(),"seconds":seconds,"texts_per_second":selected.len() as f64/seconds}));
    }
    fs::write(
        out.join("summary.json"),
        serde_json::to_vec_pretty(
            &json!({"config":cfg,"load":load,"indexing":indexing,"document_ids":selected.iter().map(|s|quality::digest(s.as_bytes())).collect::<Vec<_>>(),"document_bytes":selected.iter().map(|s|s.len()).collect::<Vec<_>>(),"rows_blake3":file_digest(&out.join("rows.jsonl"))?}),
        )?,
    )?;
    Ok(())
}

fn scale(data: &Path, models: &Path, config: &Path, out: &Path) -> Result<()> {
    let cfg: Config = serde_json::from_slice(&fs::read(config)?)?;
    cfg.validate()?;
    if out.exists() {
        return Err("scale requires new directory".into());
    }
    fs::create_dir_all(out)?;
    let inputs: Vec<Input> = serde_json::from_slice(&fs::read(data.join("inputs.json"))?)?;
    let mut model = ExperimentalEmbedder::load(models, cfg.clone())?;
    let index = Index::open(&data.join("index.sqlite"), Journal::Delete)?;
    let settings = SearchSettings::default();
    let req = SearchRequest {
        query: inputs[0].query.clone(),
        ..Default::default()
    };
    search(&index, &mut model, None, &settings, &req)?;
    let mut rows = File::create(out.join("rows.jsonl"))?;
    for repeat in 0..cfg.repeats {
        for c in &inputs {
            let req = SearchRequest {
                query: c.query.clone(),
                ..Default::default()
            };
            let start = Instant::now();
            let response = search(&index, &mut model, None, &settings, &req)?;
            emit(
                &mut rows,
                json!({"id":c.id,"repeat":repeat,"seconds":start.elapsed().as_secs_f64(),"embed":model.seconds,"hits":response.hits.len()}),
            )?;
        }
    }
    fs::write(
        out.join("complete.json"),
        serde_json::to_vec_pretty(
            &json!({"config":cfg,"chunks":index.chunk_count()?,"rows_blake3":file_digest(&out.join("rows.jsonl"))?}),
        )?,
    )?;
    Ok(())
}

fn copy_documents(source: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            copy_documents(&entry.path(), &dest.join(entry.file_name()))?;
        } else if entry.file_type()?.is_file() {
            fs::copy(entry.path(), dest.join(entry.file_name()))?;
        } else {
            return Err("unexpected snapshot symlink".into());
        }
    }
    Ok(())
}

struct SerialDocuments(Box<dyn Embedder>);
impl Embedder for SerialDocuments {
    fn fingerprint(&self) -> &ModelFingerprint {
        self.0.fingerprint()
    }
    fn execution(&self) -> bars_core::models::ExecutionInfo {
        self.0.execution()
    }
    fn embed(&mut self, texts: &[&str]) -> std::result::Result<Vec<Vec<f32>>, EmbedError> {
        let mut vectors = Vec::with_capacity(texts.len());
        for text in texts {
            let one = self.0.embed(&[*text])?;
            if one.len() != 1 {
                return Err(EmbedError::Count {
                    expected: 1,
                    got: one.len(),
                });
            }
            vectors.extend(one);
        }
        Ok(vectors)
    }
    fn embed_query(&mut self, text: &str) -> std::result::Result<Vec<f32>, EmbedError> {
        self.0.embed_query(text)
    }
}

fn index_bench(data: &Path, models: &Path, config: &Path, out: &Path, serial: bool) -> Result<()> {
    use bars_core::indexing::{
        chunking::ChunkingParams,
        indexer::{IndexSettings, UpdateOutcome, update_index},
    };
    let cfg: Config = serde_json::from_slice(&fs::read(config)?)?;
    cfg.validate()?;
    if out.exists() {
        return Err("index requires new directory".into());
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(data.join("manifest.json"))?)?;
    let source = Path::new(manifest["source"].as_str().ok_or("missing frozen source")?);
    verify_snapshot(source)?;
    let project = out.join("project");
    copy_documents(&source.join("project/docs"), &project.join("docs"))?;
    let builtin_profile = cfg.name.strip_prefix("core-");
    if serial && builtin_profile != Some("fp16") {
        return Err("serial memory experiment requires builtin FP16".into());
    }
    let native = native_memory::windows::Log::create(&out.join("native-memory.jsonl"))?;
    native.reserve(if serial { 5 } else { 8 })?;
    native.emit(
        "config",
        "index",
        json!({"builtin_profile":builtin_profile,"config":cfg,"serial_documents":serial}),
    );
    let monitor = native_memory::windows::Monitor::start(native.clone());
    let load = Instant::now();
    let mut model = load_embedding_profile(models, cfg.clone(), builtin_profile)?;
    if serial {
        model = Box::new(SerialDocuments(model));
    }
    let load = load.elapsed().as_secs_f64();
    model.embed_query("прогрев встроенной модели")?;
    let execution = model.execution();
    let fp = model.fingerprint();
    let identity = json!({"backend":execution.backend,"actual":execution.actual,"precision":execution.precision,"revision":execution.revision,
        "fingerprint":{"source":fp.source,"model":fp.model,"dim":fp.dim,"doc_prefix":fp.doc_prefix}});
    native.emit("marker", "index_model_loaded", identity.clone());
    let mut index = Index::open(&out.join("index.sqlite"), Journal::Delete)?;
    let settings = IndexSettings {
        chunking: ChunkingParams::default(),
        docs_root: "docs".into(),
    };
    let mut rows = Vec::new();
    for mode in ["full", "unchanged", "incremental"] {
        if mode == "incremental" {
            let doc = index
                .documents()?
                .into_iter()
                .next()
                .ok_or("empty corpus")?;
            let mut file = OpenOptions::new()
                .append(true)
                .open(project.join(doc.path))?;
            writeln!(
                file,
                "\n## Измерительный раздел этапа 11\n\nИзменение только копии корпуса для проверки инкрементального прохода."
            )?;
        }
        let start = Instant::now();
        let outcome = update_index(&project, &mut index, &settings, &mut *model)?;
        let seconds = start.elapsed().as_secs_f64();
        let UpdateOutcome::Updated(report) = outcome else {
            return Err("unexpected model/layout incompatibility".into());
        };
        if !report.problems.is_empty() {
            return Err(format!("index problems: {report:?}").into());
        }
        let chunks = index.chunk_count()?;
        if chunks != index.vector_count()? {
            return Err("index/vector count differs".into());
        }
        if mode == "full" {
            let frozen = out.join("full-data");
            fs::create_dir_all(&frozen)?;
            // The owned index uses DELETE journal; update_index has committed and no writer runs.
            if out.join("index.sqlite-wal").exists() || out.join("index.sqlite-journal").exists() {
                return Err("index still has a sidecar; cannot freeze by copy".into());
            }
            fs::copy(out.join("index.sqlite"), frozen.join("index.sqlite"))?;
            fs::copy(data.join("inputs.json"), frozen.join("inputs.json"))?;
            fs::copy(data.join("cases.json"), frozen.join("cases.json"))?;
            let mut frozen_manifest = manifest.clone();
            frozen_manifest["index_blake3"] = json!(file_digest(&frozen.join("index.sqlite"))?);
            frozen_manifest["embedding_config"] = serde_json::to_value(&cfg)?;
            if let Some(profile) = builtin_profile {
                frozen_manifest["builtin_embedding_profile"] = json!(profile);
            }
            fs::write(
                frozen.join("manifest.json"),
                serde_json::to_vec_pretty(&frozen_manifest)?,
            )?;
        }
        rows.push(json!({"mode":mode,"seconds":seconds,"documents":index.document_count()?,"chunks":chunks,"report":format!("{report:?}")}));
        eprintln!(
            "index {} {} {:.3}s chunks={chunks}",
            cfg.name, mode, seconds
        );
    }
    fs::write(
        out.join("complete.json"),
        serde_json::to_vec_pretty(
            &json!({"config":cfg,"serial_documents":serial,"source_manifest":manifest,"load":load,"rows":rows,"identity":identity,"index_blake3":file_digest(&out.join("index.sqlite"))?}),
        )?,
    )?;
    drop(model);
    monitor.stop()?;
    native.emit("end", "index_end", json!({"ok":true}));
    Ok(())
}

pub fn main() -> Result<()> {
    let a: Vec<_> = std::env::args().skip(1).collect();
    match a.first().map(String::as_str) {
        Some("download-embed-fp16") if a.len()==3=>{
            let native = native_memory::windows::Log::create(Path::new(&a[2]))?;
            native.reserve(2)?;
            native.emit("config", "artifact_only", json!({"model":bars_core::models::BGE_M3_FP16_ID,"binary_blake3":file_digest(&std::env::current_exe()?)?}));
            let monitor = native_memory::windows::Monitor::start(native.clone());
            let result = bars_core::models::download_onnx_artifacts(Path::new(&a[1]), bars_core::models::BGE_M3_FP16_ID);
            monitor.stop()?;
            native.emit("end", "artifact_only", json!({"ok":result.is_ok(),"ready":bars_core::models::is_downloaded(Path::new(&a[1]), bars_core::models::BGE_M3_FP16_ID)}));
            result.map_err(Into::into)
        },
        Some("embed-fingerprint") if a.len()==2=>{
            let fp=match a[1].as_str(){"fp16"=>bars_core::models::bge_m3_fp16_fingerprint(),"int8"=>bars_core::models::bge_m3_int8_fingerprint(),_=>return Err("unknown precision".into())};
            println!("{}",json!({"source":fp.source,"model":fp.model,"dim":fp.dim,"doc_prefix":fp.doc_prefix}));Ok(())
        },
        Some("digest") if a.len()==2=>{println!("{}",file_digest(Path::new(&a[1]))?);Ok(())},
        Some("verify") if a.len()==2=>verify_snapshot(Path::new(&a[1])),
        Some("prepare") if a.len()==5=>prepare(Path::new(&a[1]),Path::new(&a[2]),Path::new(&a[3]),Path::new(&a[4])),
        Some("run") if a.len()==5=>run(Path::new(&a[1]),Path::new(&a[2]),Path::new(&a[3]),Path::new(&a[4])),
        Some("profile") if a.len()==6=>profile(Path::new(&a[1]),Path::new(&a[2]),Path::new(&a[3]),Path::new(&a[4]),Path::new(&a[5])),
        Some("binding") if a.len()==6=>binding_bench(Path::new(&a[1]),Path::new(&a[2]),Path::new(&a[3]),Path::new(&a[4]),Path::new(&a[5])),
        Some("full") if a.len()==5=>full(Path::new(&a[1]),Path::new(&a[2]),Path::new(&a[3]),Path::new(&a[4]),false),
        Some("quality") if a.len()==5=>full(Path::new(&a[1]),Path::new(&a[2]),Path::new(&a[3]),Path::new(&a[4]),true),
        Some("index") if a.len()==5=>index_bench(Path::new(&a[1]),Path::new(&a[2]),Path::new(&a[3]),Path::new(&a[4]),false),
        Some("index-serial") if a.len()==5=>index_bench(Path::new(&a[1]),Path::new(&a[2]),Path::new(&a[3]),Path::new(&a[4]),true),
        Some("embed") if a.len()==5=>embed_bench(Path::new(&a[1]),Path::new(&a[2]),Path::new(&a[3]),Path::new(&a[4])),
        Some("scale") if a.len()==5=>scale(Path::new(&a[1]),Path::new(&a[2]),Path::new(&a[3]),Path::new(&a[4])),
        _=>Err("prepare <snapshot> <cases> <models> <new-data-dir> | run <data> <models> <config> <new-output-dir>".into())
    }
}

fn binding_bench(data: &Path, models: &Path, config: &Path, onnx: &Path, out: &Path) -> Result<()> {
    use ort::{
        session::{Session, builder::GraphOptimizationLevel},
        value::Tensor,
    };
    let cfg: Config = serde_json::from_slice(&fs::read(config)?)?;
    cfg.validate()?;
    if out.exists() {
        return Err("binding requires new directory".into());
    }
    fs::create_dir_all(out)?;
    let inputs: Vec<Input> = serde_json::from_slice(&fs::read(data.join("inputs.json"))?)?;
    let input = inputs
        .iter()
        .find(|c| cfg.ids.is_empty() || cfg.ids.contains(&c.id))
        .ok_or("empty binding selection")?;
    let model = ExperimentalReranker::load(models, cfg.clone())?;
    let tokenizer = model.model.tokenizer.clone();
    drop(model);
    let mut builder = Session::builder()?
        .with_execution_providers(providers(&cfg.device))?
        .with_optimization_level(GraphOptimizationLevel::Level3)?
        .with_intra_threads(cfg.threads.unwrap_or(16))?;
    if cfg.device == "directml" {
        builder = builder
            .with_memory_pattern(false)?
            .with_parallel_execution(false)?;
    }
    let mut session = builder.commit_from_file(onnx)?;
    let encoded = tokenizer
        .encode_batch(
            input
                .documents
                .iter()
                .take(cfg.batch)
                .map(|d| (input.query.as_str(), d.as_str()))
                .collect(),
            true,
        )
        .map_err(|e| e.to_string())?;
    let shape = [encoded.len(), encoded[0].len()];
    let ids = Tensor::from_array((
        shape,
        encoded
            .iter()
            .flat_map(|e| e.get_ids().iter().map(|&x| i64::from(x)))
            .collect::<Vec<_>>(),
    ))?;
    let mask = Tensor::from_array((
        shape,
        encoded
            .iter()
            .flat_map(|e| e.get_attention_mask().iter().map(|&x| i64::from(x)))
            .collect::<Vec<_>>(),
    ))?;
    if session
        .inputs()
        .iter()
        .any(|i| i.name() == "token_type_ids")
    {
        return Err("binding spike currently covers two-input BGE export".into());
    }
    let baseline =
        session.run(ort::inputs!["input_ids"=>ids.view(),"attention_mask"=>mask.view()])?;
    let (output_shape, reference) = baseline["logits"].try_extract_tensor::<f32>()?;
    let output_shape: Vec<usize> = output_shape.iter().map(|&x| x as usize).collect();
    let reference = reference.to_vec();
    drop(baseline);
    let mut resident = session.create_binding()?;
    resident.bind_input("input_ids", &ids)?;
    resident.bind_input("attention_mask", &mask)?;
    resident.bind_output(
        "logits",
        Tensor::from_array((output_shape.clone(), vec![0f32; reference.len()]))?,
    )?;
    resident.synchronize_inputs()?;
    let mut rows = Vec::new();
    for repeat in 0..20 {
        let order = if repeat % 2 == 0 {
            ["regular", "rebind", "resident"]
        } else {
            ["resident", "rebind", "regular"]
        };
        for mode in order {
            let start = Instant::now();
            let logits =
                match mode {
                    "regular" => session
                        .run(ort::inputs!["input_ids"=>ids.view(),"attention_mask"=>mask.view()])?
                        ["logits"]
                        .try_extract_tensor::<f32>()?
                        .1
                        .to_vec(),
                    "rebind" => {
                        let mut binding = session.create_binding()?;
                        binding.bind_input("input_ids", &ids)?;
                        binding.bind_input("attention_mask", &mask)?;
                        binding.bind_output(
                            "logits",
                            Tensor::from_array((
                                output_shape.clone(),
                                vec![0f32; reference.len()],
                            ))?,
                        )?;
                        binding.synchronize_inputs()?;
                        let outputs = session.run_binding(&binding)?;
                        binding.synchronize_outputs()?;
                        outputs["logits"].try_extract_tensor::<f32>()?.1.to_vec()
                    }
                    _ => {
                        let outputs = session.run_binding(&resident)?;
                        resident.synchronize_outputs()?;
                        outputs["logits"].try_extract_tensor::<f32>()?.1.to_vec()
                    }
                };
            let seconds = start.elapsed().as_secs_f64();
            if logits.len() != reference.len() || logits.iter().any(|x| !x.is_finite()) {
                return Err("invalid bound output".into());
            }
            let delta = logits
                .iter()
                .zip(&reference)
                .map(|(a, b)| (a - b).abs())
                .fold(0f32, f32::max);
            if delta > 0.001 {
                return Err(format!("bound logits differ: {delta}").into());
            }
            rows.push(
                json!({"mode":mode,"repeat":repeat,"seconds":seconds,"max_logit_delta":delta}),
            );
        }
    }
    fs::write(
        out.join("complete.json"),
        serde_json::to_vec_pretty(
            &json!({"config":cfg,"input":input.id,"shape":shape,"model_blake3":file_digest(onnx)?,"rows":rows,"limitation":"resident reuses unchanged inputs; not the new-query product path"}),
        )?,
    )?;
    Ok(())
}

fn profile(data: &Path, models: &Path, config: &Path, onnx: &Path, out: &Path) -> Result<()> {
    use ort::{
        session::{Session, builder::GraphOptimizationLevel},
        value::Tensor,
    };
    let cfg: Config = serde_json::from_slice(&fs::read(config)?)?;
    cfg.validate()?;
    if out.exists() {
        return Err("profile requires new directory".into());
    }
    fs::create_dir_all(out)?;
    let inputs: Vec<Input> = serde_json::from_slice(&fs::read(data.join("inputs.json"))?)?;
    let model = ExperimentalReranker::load(models, cfg.clone())?;
    let tokenizer = model.model.tokenizer.clone();
    drop(model);
    let mut builder = Session::builder()?
        .with_execution_providers(providers(&cfg.device))
        .map_err(|e| e.to_string())?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|e| e.to_string())?
        .with_intra_threads(
            cfg.threads
                .unwrap_or(std::thread::available_parallelism()?.get()),
        )
        .map_err(|e| e.to_string())?
        .with_profiling(out.join("ort"))
        .map_err(|e| e.to_string())?;
    if cfg.name.ends_with("-cache-write") {
        builder = builder
            .with_optimized_model_path(out.join("optimized.onnx"))
            .map_err(|e| e.to_string())?;
    }
    if cfg.device == "directml" {
        builder = builder
            .with_memory_pattern(false)
            .map_err(|e| e.to_string())?
            .with_parallel_execution(false)
            .map_err(|e| e.to_string())?;
    }
    let start = Instant::now();
    let mut session = builder.commit_from_file(onnx).map_err(|e| e.to_string())?;
    let load = start.elapsed().as_secs_f64();
    let need_types = session
        .inputs()
        .iter()
        .any(|i| i.name() == "token_type_ids");
    let mut observations = Vec::new();
    for c in inputs
        .iter()
        .filter(|c| cfg.ids.is_empty() || cfg.ids.contains(&c.id))
        .take(3)
    {
        for docs in c.documents.chunks(cfg.batch) {
            let start = Instant::now();
            let encoded = tokenizer
                .encode_batch(
                    docs.iter()
                        .map(|d| (c.query.as_str(), d.as_str()))
                        .collect(),
                    true,
                )
                .map_err(|e| e.to_string())?;
            let tokenization = start.elapsed().as_secs_f64();
            let len = encoded[0].len();
            let shape = [encoded.len(), len];
            let ids: Vec<i64> = encoded
                .iter()
                .flat_map(|e| e.get_ids().iter().map(|&x| i64::from(x)))
                .collect();
            let mask: Vec<i64> = encoded
                .iter()
                .flat_map(|e| e.get_attention_mask().iter().map(|&x| i64::from(x)))
                .collect();
            let types: Vec<i64> = encoded
                .iter()
                .flat_map(|e| e.get_type_ids().iter().map(|&x| i64::from(x)))
                .collect();
            let start = Instant::now();
            let ids = Tensor::from_array((shape, ids))?;
            let mask = Tensor::from_array((shape, mask))?;
            let tensors = start.elapsed().as_secs_f64();
            let start = Instant::now();
            let outputs = if need_types {
                session.run(ort::inputs!["input_ids"=>ids,"attention_mask"=>mask,"token_type_ids"=>Tensor::from_array((shape,types))?])?
            } else {
                session.run(ort::inputs!["input_ids"=>ids,"attention_mask"=>mask])?
            };
            let (_, logits) = outputs["logits"].try_extract_tensor::<f32>()?;
            if logits.iter().any(|x| !x.is_finite()) {
                return Err("nonfinite profile logits".into());
            }
            observations.push(json!({"id":c.id,"batch":shape,"tokenization":tokenization,"tensors":tensors,"run":start.elapsed().as_secs_f64(),"logits":logits}));
        }
    }
    let profile_path = session.end_profiling()?;
    fs::write(
        out.join("observations.json"),
        serde_json::to_vec_pretty(
            &json!({"config":cfg,"load":load,"profile":profile_path,"rows":observations}),
        )?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serial_documents_preserve_full_text_order_and_query_path() {
        struct Probe {
            fp: ModelFingerprint,
        }
        impl Embedder for Probe {
            fn fingerprint(&self) -> &ModelFingerprint {
                &self.fp
            }
            fn embed(&mut self, texts: &[&str]) -> std::result::Result<Vec<Vec<f32>>, EmbedError> {
                assert_eq!(texts.len(), 1);
                Ok(vec![vec![texts[0].chars().count() as f32]])
            }
            fn embed_query(&mut self, _: &str) -> std::result::Result<Vec<f32>, EmbedError> {
                Ok(vec![-1.0])
            }
        }
        let fp = ModelFingerprint {
            source: "test".into(),
            model: "serial".into(),
            dim: 1,
            doc_prefix: "prefix".into(),
        };
        let mut model = SerialDocuments(Box::new(Probe { fp: fp.clone() }));
        let long = "текст ".repeat(3000);
        assert_eq!(model.fingerprint(), &fp);
        assert_eq!(
            model.embed(&[&long, "я", ""]).unwrap(),
            vec![vec![18000.0], vec![1.0], vec![0.0]]
        );
        assert!(model.embed(&[]).unwrap().is_empty());
        assert_eq!(model.embed_query("запрос").unwrap(), vec![-1.0]);
    }
    #[test]
    fn permutation_keeps_equal_lengths_and_restores_original_indices() {
        let order = order_for_lengths(&[10, 2, 10, 1], true);
        assert_eq!(order, vec![3, 1, 0, 2]);
        assert_eq!(
            restore(&[(3, 40.), (0, 10.), (2, 30.), (1, 20.)], &order).unwrap(),
            vec![30., 20., 40., 10.]
        );
        assert!(restore(&[(0, 1.), (0, 2.)], &[0, 1]).is_err());
        assert!(restore(&[(0, f32::NAN)], &[0]).is_err());
        assert!(restore(&[(2, 1.)], &[0]).is_err());
        assert_eq!(restore(&[], &[]).unwrap(), Vec::<f32>::new());
    }
    #[test]
    fn invalid_experiments_are_rejected() {
        let mut cfg:Config=serde_json::from_str(r#"{"name":"test","device":"cpu","threads":null,"batch":4,"sorted":false,"max_length":512,"repeats":1}"#).unwrap();
        cfg.validate().unwrap();
        cfg.batch = 0;
        assert!(cfg.validate().is_err());
        cfg.batch = 4;
        cfg.threads = Some(0);
        assert!(cfg.validate().is_err());
    }
}
