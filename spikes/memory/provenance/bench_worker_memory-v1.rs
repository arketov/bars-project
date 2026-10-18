//! Настоящие worker slots и timer, ускоренные TTL; без рабочего профиля и IPC.
#[cfg(windows)]
#[allow(dead_code)]
#[path = "../../bars-core/examples/bench_memory.rs"]
mod standalone;

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use bars_core::{
        models::{
            Accelerator, BGE_M3_ID, is_downloaded, reranker_files_present, reranker_is_downloaded,
        },
        settings::{Settings, UserSettings},
    };
    use bars_worker::worker::{ConfiguredProvider, ModelIdlePolicy, Worker, WorkerConfig};
    use serde_json::json;
    use standalone::windows::{Log, Monitor};
    use std::{fs, path::Path, sync::Arc, time::Duration};
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 5 || args[0] != "bounded" || !matches!(args[1].as_str(), "cpu" | "auto") {
        return Err("bench_worker_memory bounded <cpu|auto> <cache> <fixture> <new-jsonl>".into());
    }
    let accelerator = if args[1] == "cpu" {
        Accelerator::Cpu
    } else {
        Accelerator::Auto
    };
    let cache = Path::new(&args[2]);
    if !is_downloaded(cache, BGE_M3_ID)
        || !reranker_files_present(cache, accelerator)
        || !reranker_is_downloaded(cache, Accelerator::Cpu)
    {
        return Err("all selected artifacts and CPU fallback must be cached".into());
    }
    // Environment setup precedes every runtime/sampler thread.
    let provider = Arc::new(ConfiguredProvider::new(cache.into()));
    let fixture: Fixture = serde_json::from_slice(&fs::read(&args[3])?)?;
    if fixture.documents.len() != 15 {
        return Err("fixture needs 15 documents".into());
    }
    let output = Path::new(&args[4]);
    let log = Log::create(output)?;
    log.preflight()?;
    let home = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../spikes/memory/worker-profiles")
        .join(output.file_stem().unwrap());
    fs::create_dir_all(home.parent().unwrap())?;
    fs::create_dir(&home)?;
    let settings_text = format!(
        "[embed]\naccelerator = \"{}\"\n[rerank]\nenabled = true\naccelerator = \"{}\"\n",
        args[1], args[1]
    );
    fs::write(home.join("settings.toml"), &settings_text)?;
    let user = UserSettings::load(&home)?.value;
    let settings = Settings::resolve(&user, &Default::default());
    let mut config = WorkerConfig::new(home);
    config.model_idle = ModelIdlePolicy {
        embed: Duration::from_secs(if accelerator == Accelerator::Cpu {
            120
        } else {
            20
        }),
        rerank: Duration::from_secs(3),
        check_interval: Duration::from_millis(100),
    };
    log.emit("config", "startup", json!({"mode":"bounded", "accelerator":args[1], "loader":"production-worker", "fixture_blake3":blake3::hash(&fs::read(&args[3])?).to_hex().to_string(), "interval_ms":100,
        "rerank_ttl_seconds":3, "embed_ttl_seconds":config.model_idle.embed.as_secs(), "check_interval_ms":100}));
    let monitor = Monitor::start(log.clone());
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let result = runtime.block_on(async {
        let worker = Worker::start(config, provider)?;
        let result = exercise(&worker, &settings, &settings_text, &fixture, &log).await;
        worker.shutdown().await;
        drop(worker);
        log.plateau("worker_shutdown");
        result
    });
    let sampled = monitor.stop();
    let result = result.and(sampled);
    log.emit(
        "end",
        "process_end",
        json!({"ok":result.is_ok(), "error":result.as_ref().err().map(ToString::to_string)}),
    );
    result
}

#[cfg(windows)]
#[derive(serde::Deserialize)]
struct Fixture {
    query: String,
    documents: Vec<String>,
}

#[cfg(windows)]
async fn wait(
    mut condition: impl FnMut() -> bool,
    seconds: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    tokio::time::timeout(std::time::Duration::from_secs(seconds), async {
        while !condition() {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .map_err(|_| "worker lifecycle timeout".into())
}

#[cfg(windows)]
async fn exercise(
    worker: &bars_worker::worker::Worker,
    settings: &bars_core::settings::Settings,
    settings_text: &str,
    fixture: &Fixture,
    log: &standalone::windows::Log,
) -> Result<(), Box<dyn std::error::Error>> {
    use bars_core::models::{Embedder, Reranker};
    use bars_worker::worker::Readiness;
    use serde_json::json;
    use std::time::Instant;
    let texts: Vec<&str> = fixture.documents.iter().map(String::as_str).collect();
    log.plateau("baseline");
    let mut embed = worker.models().embedder(settings)?;
    let start = Instant::now();
    embed.prepare()?;
    log.emit(
        "timing",
        "embed_load",
        json!({"ms":start.elapsed().as_secs_f64()*1000.0}),
    );
    let _ = embed.embed_query(&fixture.query)?;
    log.reserve(6)?;
    let start = Instant::now();
    drop(embed.embed(&texts[..4])?); // Actual production batch/padded bound, not its imitation.
    log.emit(
        "timing",
        "embed_batch4",
        json!({"ms":start.elapsed().as_secs_f64()*1000.0}),
    );
    log.plateau("embed_batch_retained");
    log.reserve(6)?;
    let start = Instant::now();
    loop {
        match worker.models().rerank_readiness(settings) {
            Readiness::Ready => break,
            Readiness::Failed(message) => return Err(message.into()),
            Readiness::Loading { .. } => {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await
            }
        }
        if start.elapsed().as_secs() > 60 {
            return Err("cached rerank load timed out".into());
        }
    }
    let actual = worker.model_runtime().ok_or("missing runtime")?.rerank;
    if settings.rerank.accelerator == bars_core::models::Accelerator::Auto
        && actual.actual.as_deref() != Some("directml")
    {
        return Err("unexpected CPU fallback in GPU lifecycle acceptance".into());
    }
    log.emit(
        "timing",
        "rerank_load",
        json!({"ms":start.elapsed().as_secs_f64()*1000.0,"runtime":actual}),
    );
    let mut rerank = worker.models().reranker(settings);
    let mut reference = Vec::new();
    for n in 0..3 {
        let start = Instant::now();
        let scores = rerank.rerank(&fixture.query, &texts)?;
        log.emit(
            "timing",
            "rerank",
            json!({"n":n, "ms":start.elapsed().as_secs_f64()*1000.0,"scores":scores}),
        );
        reference = scores;
    }
    log.plateau("rerank_retained");
    wait(|| !rerank.slot().is_loaded(), 15).await?;
    if !embed.slot().is_loaded() {
        return Err("embed retired before rerank proof".into());
    }
    log.plateau("rerank_dropped");
    log.reserve(6)?;
    let start = Instant::now();
    let scores = rerank.rerank(&fixture.query, &texts)?;
    let delta = scores
        .iter()
        .zip(&reference)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    if scores.len() != 15 || scores.iter().any(|v| !v.is_finite()) || delta > 1e-5 {
        return Err("worker reload changed scores".into());
    }
    log.emit("timing", "rerank_after_reload", json!({"ms":start.elapsed().as_secs_f64()*1000.0,"scores":scores,"max_score_delta":delta,"includes_load":true}));
    let start = Instant::now();
    std::fs::write(
        worker.bars_home().join("settings.toml"),
        settings_text.replace("enabled = true", "enabled = false"),
    )?;
    wait(|| !rerank.slot().is_loaded(), 2).await?;
    log.emit(
        "timing",
        "disable_rerank",
        json!({"ms":start.elapsed().as_secs_f64()*1000.0}),
    );
    log.plateau("rerank_second_drop");
    wait(|| !embed.slot().is_loaded(), 130).await?;
    log.plateau("all_models_dropped");
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    panic!("Windows memory counters required");
}
