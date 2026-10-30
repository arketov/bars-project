//! 4.2.1: настоящий Worker/IPC, замороженный корпус, отдельный процесс на серию.
#[cfg(windows)]
#[allow(dead_code)]
#[path = "../../bars-core/examples/bench_memory.rs"]
mod memory;

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use bars_core::models::{
        Accelerator, BGE_M3_FP16_ID, BGE_M3_ID, is_downloaded, reranker_is_downloaded,
    };
    use bars_worker::worker::{ConfiguredProvider, Worker, WorkerConfig};
    use memory::windows::{Log, Monitor};
    use serde_json::json;
    use std::{
        fs,
        io::Write,
        path::PathBuf,
        sync::{Arc, Mutex},
        time::Duration,
    };
    let a: Vec<_> = std::env::args().skip(1).collect();
    if !(a.len() == 6 || a.len() == 7)
        || !matches!(a[0].as_str(), "fp32" | "fp16")
        || !matches!(a[1].as_str(), "on" | "off")
        || !matches!(a[2].as_str(), "1" | "2" | "4")
        || a.get(6)
            .is_some_and(|mode| !matches!(mode.as_str(), "overlap" | "serial"))
    {
        return Err("bench_worker_index_memory <fp32|fp16> <on|off rerank> <1|2|4 clients> <models> <copied-project> <new-run-dir> [overlap|serial]".into());
    }
    let embed = if a[0] == "fp16" {
        BGE_M3_FP16_ID
    } else {
        BGE_M3_ID
    };
    let rerank = a[1] == "on";
    let clients: usize = a[2].parse()?;
    let models = PathBuf::from(&a[3]);
    if !is_downloaded(&models, embed)
        || (rerank
            && (!reranker_is_downloaded(&models, Accelerator::Auto)
                || !reranker_is_downloaded(&models, Accelerator::Cpu)))
    {
        return Err("selected artifacts must be cached; no download allowed".into());
    }
    let project = PathBuf::from(&a[4]).canonicalize()?;
    let run = PathBuf::from(&a[5]);
    fs::create_dir(&run)?;
    let log = Log::create(&run.join("memory.jsonl"))?;
    log.reserve(8)?;
    // Environment and ORT logger configured before all runtime/sampler threads.
    unsafe {
        std::env::set_var("ORT_LOG", "verbose");
    }
    let ort_log = Arc::new(Mutex::new(fs::File::create(run.join("ort.jsonl"))?));
    let clock = std::time::Instant::now();
    if !ort::init()
        .with_logger(Arc::new(move |level, category, id, location, message| {
            // Raw ORT events: do not mistake tensor-size metadata for live allocator bytes.
            let row = json!({"ms":clock.elapsed().as_secs_f64()*1000.0,"level":format!("{level:?}"),
            "category":category,"session":id,"location":location,"message":message});
            let _ = writeln!(ort_log.lock().unwrap(), "{row}");
        }))
        .commit()
    {
        return Err("ORT environment was already configured".into());
    }
    let home = run.join("home");
    fs::create_dir(&home)?;
    fs::write(
        home.join("settings.toml"),
        format!(
            "[embed]\nmodel = \"{embed}\"\naccelerator = \"auto\"\n[rerank]\nenabled = {rerank}\naccelerator = \"auto\"\n"
        ),
    )?;
    let provider = Arc::new(ConfiguredProvider::new(models));
    let mut config = WorkerConfig::new(home);
    config.periodic_interval = Duration::from_secs(3600);
    let monitor = Monitor::start(log.clone());
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()?;
    let result = runtime.block_on(async {
        let worker = Worker::start(config, provider)?;
        let result = tokio::time::timeout(
            Duration::from_secs(1200),
            exercise(
                &worker,
                &project,
                clients,
                rerank,
                &a[0],
                a.get(6).is_some_and(|mode| mode == "serial"),
                &log,
            ),
        )
        .await;
        worker.shutdown().await;
        drop(worker);
        log.plateau("shutdown");
        match result {
            Ok(r) => r,
            Err(_) => Err("series timed out".into()),
        }
    });
    let result = result.and(monitor.stop());
    log.emit(
        "end",
        "end",
        json!({"ok":result.is_ok(),"error":result.as_ref().err().map(ToString::to_string)}),
    );
    result
}

#[cfg(windows)]
async fn exercise(
    worker: &bars_worker::worker::Worker,
    root: &std::path::Path,
    clients: usize,
    rerank: bool,
    precision: &str,
    serial: bool,
    log: &memory::windows::Log,
) -> Result<(), Box<dyn std::error::Error>> {
    use bars_protocol::{Client, ClientKind, Endpoint, ScoreKind, SearchParams, Target};
    use bars_worker::handler::WorkerHandler;
    use serde_json::json;
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    let endpoint = Endpoint::named(&format!("memory-421-{}", std::process::id()));
    let listener = bars_worker::ipc::bind(&endpoint)?;
    let server = tokio::spawn(bars_worker::ipc::run_server(
        listener,
        Arc::new(WorkerHandler::new(worker.clone())),
    ));
    // Abort the listener on every exit; no detached server after a failed assertion.
    struct Server(tokio::task::JoinHandle<std::io::Result<()>>);
    impl Drop for Server {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _server = Server(server);
    log.plateau("baseline");
    worker.register(root)?;
    while !worker.is_idle() {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let status = worker.status(root)?;
    if status.last_error.is_some() {
        return Err(format!("initial indexing: {status:?}").into());
    }
    log.emit(
        "status",
        "initial_index_done",
        json!({"snapshot":format!("{status:?}"),"last_error":status.last_error}),
    );
    let queries = [
        "Как bars режет документы на чанки?",
        "Как устроена очередь индексации?",
        "Как выбирается модель rerank?",
        "Как работает гибридный поиск?",
    ];
    let params: Vec<_> = queries
        .iter()
        .map(|query| SearchParams {
            diagnostic: true,
            target: Target {
                cwd: root.display().to_string(),
                project: None,
            },
            query: (*query).into(),
            modules: vec![],
            files: vec![],
            limit: Some(5),
            candidates: Some(30),
            min_score: Some(0.0),
        })
        .collect();
    let expected_kind = if rerank {
        ScoreKind::Rerank
    } else {
        ScoreKind::Cosine
    };
    let mut warm = Client::connect(&endpoint, ClientKind::Mcp).await?;
    // An unchanged persisted index does not load a model at registration.
    // Prepare slots before IPC warmup instead of retrying failed searches.
    use bars_core::models::Embedder;
    use bars_worker::worker::Readiness;
    let settings = bars_core::settings::Settings::load(worker.bars_home(), root)?.value;
    worker.models().embedder(&settings)?.prepare()?;
    log.plateau("embed_loaded");
    if rerank {
        tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                match worker.models().rerank_readiness(&settings) {
                    Readiness::Ready => return Ok::<(), Box<dyn std::error::Error>>(()),
                    Readiness::Failed(message) => return Err(message.into()),
                    Readiness::Loading { .. } => {
                        tokio::time::sleep(Duration::from_millis(50)).await
                    }
                }
            }
        })
        .await??;
    }
    let mut expected = Vec::new();
    for p in &params {
        let out = warm.call(p).await?;
        if out.results.is_empty()
            || out.applied.score_kind != expected_kind
            || out.rerank_error.is_some()
        {
            return Err("warmup failed after model readiness".into());
        }
        expected.push(out.results);
    }
    let runtime = worker.model_runtime().ok_or("missing runtime")?;
    if runtime.embed.actual.as_deref() != Some("directml")
        || runtime.embed.precision.as_deref() != Some(precision)
        || (rerank
            && (runtime.rerank.actual.as_deref() != Some("directml")
                || runtime.rerank.precision.as_deref() != Some("fp16")))
    {
        return Err(format!("unexpected runtime: {runtime:?}").into());
    }
    log.emit("runtime", "warm", json!(runtime));
    log.plateau("both_warm");
    let mut connections = Vec::new();
    for _ in 0..clients {
        connections.push(Client::connect(&endpoint, ClientKind::Mcp).await?);
    }
    let barrier = Arc::new(tokio::sync::Barrier::new(clients + 1));
    let mut tasks = tokio::task::JoinSet::new();
    for (lane, mut client) in connections.into_iter().enumerate() {
        let barrier = barrier.clone();
        let params = params.clone();
        let expected = expected.clone();
        let worker = worker.clone();
        let log = log.clone();
        tasks.spawn(async move {
            barrier.wait().await;
            let mut failures = 0;
            for n in 0..(100 / clients) {
                let q = (lane + n) % params.len();
                let progress_before = worker.progress();
                let before = progress_before.as_ref().is_some_and(|p| p.current_file.is_some());
                let start = Instant::now();
                let result = client.call(&params[q]).await;
                let progress_after = worker.progress();
                let after = progress_after.as_ref().is_some_and(|p| p.current_file.is_some());
                match result {
                    Ok(out) => {
                        let stable = out.results == expected[q];
                        let ok = out.rerank_error.is_none() && out.applied.score_kind == expected_kind && stable;
                        failures += usize::from(!ok);
                        log.emit("search", if serial { "after_index" } else { "during_index" }, json!({"lane":lane,"n":n,"query":q,
                            "ms":start.elapsed().as_secs_f64()*1000.0,"before":before,"after":after,
                            "indexing":out.indexing,"hits":out.results.len(),"stable":stable,
                            "docs_before":progress_before.as_ref().map(|p|p.docs_done),
                            "docs_after":progress_after.as_ref().map(|p|p.docs_done),
                            "chunks_before":progress_before.as_ref().map(|p|p.chunks_done),
                            "chunks_after":progress_after.as_ref().map(|p|p.chunks_done),
                            "score_kind":out.applied.score_kind,"rerank_error":out.rerank_error,"ok":ok}));
                    }
                    Err(e) => { failures += 1; log.emit("search", if serial { "after_index" } else { "during_index" }, json!({"lane":lane,"n":n,
                        "ms":start.elapsed().as_secs_f64()*1000.0,"before":before,"after":after,"ok":false,"error":e.to_string()})); }
                }
            }
            failures
        });
    }
    log.emit(
        "marker",
        "reindex_start",
        json!({"clients":clients,"rerank":rerank,"requests":100,"mode":if serial {"serial"} else {"overlap"}}),
    );
    let start = Instant::now();
    worker.reindex(root, true, false)?;
    if !serial {
        while !worker.progress().is_some_and(|p| p.current_file.is_some()) {
            if worker.is_idle() {
                return Err("reindex completed before overlap could be observed".into());
            }
            tokio::task::yield_now().await;
        }
    }
    // Observe index completion independently of the search series.
    let index_worker = worker.clone();
    let index_log = log.clone();
    let mut completion = tokio::spawn(async move {
        while !index_worker.is_idle() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        index_log.emit(
            "timing",
            "reindex_done",
            json!({"ms":start.elapsed().as_secs_f64()*1000.0}),
        );
    });
    if serial {
        (&mut completion).await?;
        log.plateau("serial_index_done");
    }
    log.emit("marker", "queries_start", json!({"serial":serial}));
    barrier.wait().await;
    let mut failures = 0;
    while let Some(task) = tasks.join_next().await {
        failures += task?;
    }
    if !serial {
        completion.await?;
    }
    let status = worker.status(root)?;
    log.emit(
        "status",
        "final",
        json!({"snapshot":format!("{status:?}"),"last_error":status.last_error}),
    );
    log.plateau("series_done");
    if failures > 0 || status.last_error.is_some() {
        return Err(format!(
            "{failures} failed or changed searches; index error {:?}",
            status.last_error
        )
        .into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Windows-only memory experiment");
}
