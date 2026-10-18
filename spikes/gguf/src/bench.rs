use bars_core::{
    indexing::{
        chunking::ChunkingParams,
        indexer::{IndexSettings, UpdateOutcome, update_index},
    },
    models::{Accelerator, BuiltinEmbedder, Embedder, ModelFingerprint},
    search::{SearchRequest, SearchSettings, search},
    storage::{Index, Journal},
};
use llama_cpp_2::{
    context::params::{LlamaContextParams, LlamaPoolingType},
    llama_backend::LlamaBackend,
    model::{LlamaModel, params::LlamaModelParams},
};
use std::{
    fs::{self, File},
    io::Write,
    num::NonZeroU32,
    path::Path,
    time::Instant,
};
struct Timed<'a> {
    model: &'a mut dyn Embedder,
    seconds: f64,
}
impl Embedder for Timed<'_> {
    fn fingerprint(&self) -> &ModelFingerprint {
        self.model.fingerprint()
    }
    fn embed(&mut self, t: &[&str]) -> Result<Vec<Vec<f32>>, bars_core::models::EmbedError> {
        self.model.embed(t)
    }
    fn embed_query(&mut self, t: &str) -> Result<Vec<f32>, bars_core::models::EmbedError> {
        let start = Instant::now();
        let result = self.model.embed_query(t);
        self.seconds += start.elapsed().as_secs_f64();
        result
    }
}
#[path = "../../../crates/bars-core/examples/quality/mod.rs"]
#[allow(dead_code)]
mod quality;

pub fn run(a: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if a.len() != 5 && !(a.len() == 6 && a[1].starts_with("prod-")) {
        return Err(
            "bench <gguf|bge-models-dir> <cpu|gpu|bge> <frozen-project> <cases.json> <new-output>"
                .into(),
        );
    }
    let out = Path::new(&a[4]);
    if out.join("config.json").exists() || out.join("rows.jsonl").exists() {
        return Err("завершённый output нельзя перезаписать".into());
    }
    let cases = quality::load_cases(Path::new(&a[3]))?;
    quality::validate_cases(Path::new(&a[2]), &cases)?;
    fs::create_dir_all(out)?;
    let start = Instant::now();
    if a[1] == "prod-cpu" || a[1] == "prod-gpu" {
        let mut model = bars_core::models::load_builtin_embedder(
            &a[0],
            Path::new(&a[5]),
            if a[1] == "prod-cpu" {
                Accelerator::Cpu
            } else {
                Accelerator::Auto
            },
        )?;
        return measure(a, &cases, model.as_mut(), start.elapsed().as_secs_f64());
    }
    if a[1] == "bge" {
        let mut model = BuiltinEmbedder::bge_m3(Path::new(&a[0]), Accelerator::Auto)?;
        return measure(a, &cases, &mut model, start.elapsed().as_secs_f64());
    }
    if !["cpu", "gpu"].contains(&a[1].as_str()) {
        return Err("неподдерживаемое устройство".into());
    }
    let backend = LlamaBackend::init()?;
    if a[1] == "gpu" && !backend.supports_gpu_offload() {
        return Err("Vulkan-устройство недоступно".into());
    }
    let model = LlamaModel::load_from_file(
        &backend,
        &a[0],
        &LlamaModelParams::default().with_n_gpu_layers(if a[1] == "gpu" { 1000 } else { 0 }),
    )?;
    let ctx = model.new_context(
        &backend,
        LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(8192))
            .with_n_batch(8192)
            .with_n_ubatch(512)
            .with_n_threads(8)
            .with_n_threads_batch(8)
            .with_embeddings(true)
            .with_pooling_type(LlamaPoolingType::Last),
    )?;
    let mut embed = super::embed::Gguf {
        model: &model,
        ctx,
        fp: ModelFingerprint {
            source: "spike-gguf".into(),
            model: quality::file_digest(Path::new(&a[0]))?,
            dim: model.n_embd() as usize,
            doc_prefix: "".into(),
        },
    };
    measure(a, &cases, &mut embed, start.elapsed().as_secs_f64())
}
fn measure(
    a: &[String],
    cases: &[quality::Case],
    model: &mut dyn Embedder,
    load: f64,
) -> Result<(), Box<dyn std::error::Error>> {
    let out = Path::new(&a[4]);
    let resumed = out.join("index.sqlite").exists();
    let mut index = Index::open(&out.join("index.sqlite"), Journal::Delete)?;
    let start = Instant::now();
    let result = update_index(
        Path::new(&a[2]),
        &mut index,
        &IndexSettings {
            docs_root: "docs".into(),
            chunking: ChunkingParams::default(),
        },
        model,
    )?;
    let UpdateOutcome::Updated(report) = result else {
        return Err(format!("{result:?}").into());
    };
    if !report.problems.is_empty() {
        return Err(format!("{:?}", report.problems).into());
    }
    let index_seconds = start.elapsed().as_secs_f64();
    fs::write(
        out.join("config.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"args":a,"load_seconds":load,"index_seconds":index_seconds,"resumed_index":resumed,"cases_blake3":quality::file_digest(Path::new(&a[3]))?,"binary_blake3":quality::file_digest(&std::env::current_exe()?)?,"report":format!("{report:?}")}),
        )?,
    )?;
    let settings = SearchSettings {
        results: 30,
        max_results: 30,
        min_score_vector: 0.0,
        ..Default::default()
    };
    model.embed_query("порядок запуска ботов")?;
    let mut output = File::create(out.join("rows.jsonl"))?;
    for repeat in 0..3 {
        for case in cases {
            let mut timed = Timed {
                model,
                seconds: 0.0,
            };
            let start = Instant::now();
            let response = search(
                &index,
                &mut timed,
                None,
                &settings,
                &SearchRequest {
                    query: case.query.clone(),
                    ..Default::default()
                },
            )?;
            let total = start.elapsed().as_secs_f64();
            writeln!(
                output,
                "{}",
                serde_json::to_string(&quality::Row {
                    id: case.id.clone(),
                    candidates: 30,
                    k: 0,
                    repeat,
                    mode: "search".into(),
                    total,
                    embed: timed.seconds,
                    rerank: 0.0,
                    hits: quality::pack_hits(&response.hits, case)
                })?
            )?;
            output.flush()?;
        }
    }
    Ok(())
}
