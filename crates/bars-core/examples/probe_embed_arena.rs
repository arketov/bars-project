//! ISS-18: same FP32 model/texts, one allocator/optimizer option changes; no profile/IPC writes.
#[cfg(windows)]
#[allow(dead_code)]
#[path = "bench_memory.rs"]
mod memory;

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
    use memory::windows::{Log, Monitor};
    use serde_json::json;
    use std::{path::Path, time::Instant};
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 5
        || !matches!(
            args[0].as_str(),
            "arena" | "no-arena" | "device-weights" | "no-prepack" | "no-fusion"
        )
        || !matches!(args[1].as_str(), "cpu" | "auto")
    {
        return Err("probe_embed_arena <arena|no-arena|device-weights|no-prepack|no-fusion> <cpu|auto> <cache> <fixture> <new-jsonl>".into());
    }
    let cache = Path::new(&args[2]);
    bars_core::models::prepare_environment(cache);
    if !bars_core::models::is_downloaded(cache, bars_core::models::BGE_M3_ID) {
        return Err("cached FP32 bge-m3 required; no downloads".into());
    }
    let fixture: serde_json::Value = serde_json::from_slice(&std::fs::read(&args[3])?)?;
    let docs: Vec<&str> = fixture["documents"]
        .as_array()
        .ok_or("documents missing")?
        .iter()
        .map(|v| v.as_str().ok_or("text missing"))
        .collect::<Result<_, _>>()?;
    let log = Log::create(Path::new(&args[4]))?;
    // Single FP32 session: measured peak <5 GiB, plus >2 GiB startup margin.
    // This does not reduce reserves for pair-model or unbounded-shape probes.
    log.reserve(7)?;
    let monitor = Monitor::start(log.clone());
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        log.plateau("baseline");
        let mut providers = Vec::new();
        if args[1] == "auto" {
            providers.push(
                ort::ep::DirectML::default()
                    .with_device_id(0)
                    .build()
                    .error_on_failure(),
            );
        }
        providers.push(
            ort::ep::CPU::default()
                .with_arena_allocator(args[0] != "no-arena")
                .build()
                .error_on_failure(),
        );
        let started = Instant::now();
        let mut options = TextInitOptions::new(EmbeddingModel::BGEM3)
            .with_cache_dir(cache.into())
            .with_max_length(8192)
            .with_execution_providers(providers);
        if args[0] == "device-weights" {
            options =
                options.with_session_config("session.use_device_allocator_for_initializers", "1");
        }
        if args[0] == "no-prepack" {
            options = options.with_session_config("session.disable_prepacking", "1");
        }
        if args[0] == "no-fusion" {
            options = options.with_session_config("ep.dml.disable_graph_fusion", "1");
        }
        let mut model = TextEmbedding::try_new(options)?;
        log.emit(
            "timing",
            "load",
            json!({"ms":started.elapsed().as_millis(),"arena":args[0],"device":args[1]}),
        );
        log.plateau("loaded");
        let mut all = Vec::new();
        for round in 0..2 {
            let started = Instant::now();
            let mut vectors = Vec::new();
            for text in &docs {
                // Startup reserves 7 GiB for a measured lifetime peak <5 GiB.
                // Repeated shapes keep a 2 GiB reserve; sampler aborts below 1 GiB.
                log.reserve(2)?;
                // A long chunk is admitted alone by the production worker too.
                vectors.extend(model.embed([*text], Some(1))?);
            }
            log.emit(
                "timing",
                "documents",
                json!({"round":round,"ms":started.elapsed().as_millis(),"count":vectors.len()}),
            );
            log.plateau("documents_done");
            all.push(vectors);
        }
        let started = Instant::now();
        let query = model.embed([fixture["query"].as_str().ok_or("query missing")?], Some(1))?;
        log.emit(
            "timing",
            "query",
            json!({"ms":started.elapsed().as_millis()}),
        );
        log.plateau("query_done");
        std::fs::write(
            format!("{}.vectors.json", args[4]),
            serde_json::to_vec(&json!({"documents":all,"query":query}))?,
        )?;
        drop(model);
        log.plateau("dropped");
        Ok(())
    })();
    let sampled = monitor.stop();
    let result = result.and(sampled);
    log.emit(
        "end",
        "end",
        json!({"ok":result.is_ok(),"error":result.as_ref().err().map(ToString::to_string)}),
    );
    result
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Windows-only memory experiment");
}
