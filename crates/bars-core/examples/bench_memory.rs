//! Windows memory/lifecycle spike; independent process, no worker IPC or user settings.
#[cfg(windows)]
pub(crate) mod windows {
    use bars_core::models::{Accelerator, BuiltinEmbedder, BuiltinReranker, Embedder, Reranker};
    use serde::Deserialize;
    use serde_json::{Value, json};
    use std::ffi::c_void;
    use std::fs::{File, OpenOptions};
    use std::io::Write;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
    const GIB: u64 = 1 << 30;
    #[repr(C)]
    #[derive(Default)]
    struct ProcessMemory {
        cb: u32,
        faults: u32,
        peak_ws: usize,
        ws: usize,
        peak_paged: usize,
        paged: usize,
        peak_nonpaged: usize,
        nonpaged: usize,
        pagefile: usize,
        peak_pagefile: usize,
        private: usize,
    }
    #[repr(C)]
    #[derive(Default)]
    struct Performance {
        cb: u32,
        commit: usize,
        limit: usize,
        peak: usize,
        physical: usize,
        available: usize,
        cache: usize,
        kernel: usize,
        kernel_paged: usize,
        kernel_nonpaged: usize,
        page_size: usize,
        handles: u32,
        processes: u32,
        threads: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
    }
    #[link(name = "psapi")]
    unsafe extern "system" {
        fn GetProcessMemoryInfo(
            process: *mut c_void,
            counters: *mut ProcessMemory,
            size: u32,
        ) -> i32;
        fn GetPerformanceInfo(info: *mut Performance, size: u32) -> i32;
    }
    fn memory() -> Value {
        let mut p = ProcessMemory {
            cb: size_of::<ProcessMemory>() as u32,
            ..Default::default()
        };
        let mut s = Performance {
            cb: size_of::<Performance>() as u32,
            ..Default::default()
        };
        // Both buffers match the documented Windows x64 layouts; API fills exactly cb bytes.
        unsafe {
            assert_ne!(
                GetProcessMemoryInfo(
                    GetCurrentProcess(),
                    &mut p,
                    size_of::<ProcessMemory>() as u32
                ),
                0
            );
            assert_ne!(
                GetPerformanceInfo(&mut s, size_of::<Performance>() as u32),
                0
            );
        }
        json!({"private_bytes":p.private, "working_set_bytes":p.ws,
            "lifetime_peak_working_set_bytes":p.peak_ws, "lifetime_peak_commit_bytes":p.peak_pagefile,
            "system_available_bytes":s.available*s.page_size,
            "system_committed_bytes":s.commit*s.page_size,
            "system_commit_limit_bytes":s.limit*s.page_size})
    }
    fn headroom(m: &Value, ram: u64, commit: u64) -> bool {
        m["system_available_bytes"].as_u64().unwrap() >= ram
            && m["system_commit_limit_bytes"].as_u64().unwrap()
                - m["system_committed_bytes"].as_u64().unwrap()
                >= commit
    }
    #[derive(Clone)]
    pub(crate) struct Log {
        file: Arc<Mutex<File>>,
        start: Instant,
    }
    impl Log {
        pub(crate) fn create(path: &Path) -> Result<Self> {
            Ok(Self {
                file: Arc::new(Mutex::new(
                    OpenOptions::new().write(true).create_new(true).open(path)?,
                )),
                start: Instant::now(),
            })
        }
        pub(crate) fn emit(&self, kind: &str, phase: &str, detail: Value) {
            let row = json!({"kind":kind, "phase":phase, "elapsed_ms":self.start.elapsed().as_secs_f64()*1000.0,
                "unix_ms":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis(),
                "pid":std::process::id(), "memory":memory(), "detail":detail});
            let mut file = self.file.lock().unwrap();
            writeln!(file, "{row}").unwrap();
            file.flush().unwrap();
        }
        pub(crate) fn plateau(&self, phase: &str) {
            self.emit("marker", phase, json!(null));
            std::thread::sleep(Duration::from_secs(2));
            self.emit("plateau_end", phase, json!(null));
        }
        pub(crate) fn preflight(&self) -> Result<()> {
            self.reserve(8)
        }
        pub(crate) fn reserve(&self, commit_gib: u64) -> Result<()> {
            if !headroom(&memory(), 4 * GIB, commit_gib * GIB) {
                self.emit(
                    "abort",
                    "preflight",
                    json!({"min_ram_gib":4,"min_commit_gib":commit_gib}),
                );
                return Err("insufficient headroom".into());
            }
            Ok(())
        }
    }
    pub(crate) struct Monitor {
        done: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }
    impl Monitor {
        pub(crate) fn start(log: Log) -> Self {
            let done = Arc::new(AtomicBool::new(false));
            let flag = Arc::clone(&done);
            let thread = std::thread::spawn(move || {
                while !flag.load(Ordering::Relaxed) {
                    if !headroom(&memory(), GIB / 2, GIB) {
                        log.emit(
                            "abort",
                            "low_memory",
                            json!("RAM <512 MiB or commit headroom <1 GiB"),
                        );
                        std::process::exit(75);
                    }
                    log.emit("sample", "monitor", json!(null));
                    std::thread::sleep(Duration::from_millis(100));
                }
            });
            Self {
                done,
                thread: Some(thread),
            }
        }
        pub(crate) fn stop(mut self) -> Result<()> {
            self.done.store(true, Ordering::Relaxed);
            self.thread
                .take()
                .unwrap()
                .join()
                .map_err(|_| "memory sampler panicked".into())
        }
    }
    impl Drop for Monitor {
        fn drop(&mut self) {
            self.done.store(true, Ordering::Relaxed);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
    #[derive(Deserialize)]
    struct Fixture {
        query: String,
        documents: Vec<String>,
    }
    pub fn run() -> Result<()> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.len() < 5
            || args.len() > 7
            || !args[5..]
                .iter()
                .all(|a| matches!(a.as_str(), "--allow-download" | "--fp16-embed"))
            || !matches!(args[0].as_str(), "embed" | "rerank" | "both" | "bounded")
            || !matches!(args[1].as_str(), "cpu" | "auto")
        {
            return Err("bench_memory <embed|rerank|both|bounded> <cpu|auto> <models> <fixture.json> <new-output.jsonl> [--allow-download] [--fp16-embed]".into());
        }
        // The fixture is frozen by the runner, and no settings/index/client files are opened.
        let fixture: Fixture = serde_json::from_slice(&std::fs::read(&args[3])?)?;
        if fixture.documents.len() != 15 {
            return Err("fixture must contain 15 documents".into());
        }
        let cache = Path::new(&args[2]);
        let allow_download = args.iter().any(|a| a == "--allow-download");
        let fp16_embed = args.iter().any(|a| a == "--fp16-embed");
        if fp16_embed && (args[1] != "auto" || args[0] == "rerank") {
            return Err("--fp16-embed requires auto and an embed mode".into());
        }
        bars_core::models::prepare_environment(cache);
        let accelerator = if args[1] == "cpu" {
            Accelerator::Cpu
        } else {
            Accelerator::Auto
        };
        if !allow_download
            && args[0] != "rerank"
            && !bars_core::models::is_downloaded(
                cache,
                if fp16_embed {
                    bars_core::models::BGE_M3_FP16_ID
                } else {
                    bars_core::models::BGE_M3_ID
                },
            )
        {
            return Err("embed model is not fully cached, refusing download".into());
        }
        if !allow_download
            && args[0] != "embed"
            && !bars_core::models::reranker_files_present(cache, accelerator)
        {
            return Err("selected rerank variant is not fully cached, refusing download".into());
        }
        // Auto may fall back after EP registration or graph creation. This spike must
        // never turn that fallback into an unmeasured network download.
        if !allow_download
            && args[0] != "embed"
            && !bars_core::models::reranker_is_downloaded(cache, Accelerator::Cpu)
        {
            return Err("CPU fallback is not fully cached, refusing download".into());
        }
        let log = Log::create(Path::new(&args[4]))?;
        log.reserve(if args[0] == "rerank" { 6 } else { 8 })?;
        log.emit("config", "startup", json!({"mode":args[0], "accelerator":args[1], "fixture_blake3":blake3::hash(&std::fs::read(&args[3])?).to_hex().to_string(), "interval_ms":100,
            "loader":"production", "embed_profile":if fp16_embed {"fp16"} else {"fp32"}, "startup_min_commit_gib":if args[0] == "rerank" {6} else {8}, "download_allowed":allow_download, "gpu_rerank_profile":"fp16-b16", "cpu_rerank_profile":"fp32-b4"}));
        let monitor = Monitor::start(log.clone());
        let result = exercise(&args, &fixture, &log);
        let sampled = monitor.stop();
        let result = result.and(sampled);
        log.emit(
            "end",
            "process_end",
            json!({"ok":result.is_ok(), "error":result.as_ref().err().map(ToString::to_string)}),
        );
        result
    }
    fn exercise(args: &[String], fixture: &Fixture, log: &Log) -> Result<()> {
        let accelerator = if args[1] == "cpu" {
            Accelerator::Cpu
        } else {
            Accelerator::Auto
        };
        let models = Path::new(&args[2]);
        let texts: Vec<&str> = fixture.documents.iter().map(String::as_str).collect();
        log.plateau("baseline");
        let mut embed = None;
        if args[0] != "rerank" {
            log.preflight()?;
            log.emit("marker", "embed_load_begin", json!(null));
            let started = Instant::now();
            let mut model = if args.iter().any(|a| a == "--fp16-embed") {
                BuiltinEmbedder::bge_m3_fp16(models, accelerator)?
            } else {
                BuiltinEmbedder::bge_m3(models, accelerator)?
            };
            let runtime = model.execution();
            log.emit("timing", "embed_load", json!({"ms":started.elapsed().as_secs_f64()*1000.0,"device":format!("{:?}",model.device()),"precision":runtime.precision,"revision":runtime.revision}));
            log.plateau("embed_loaded");
            for n in 0..3 {
                let started = Instant::now();
                let vector = model.embed_query(&fixture.query)?;
                log.emit(
                    "timing",
                    "embed_query",
                    json!({"n":n,"ms":started.elapsed().as_secs_f64()*1000.0,"dim":vector.len()}),
                );
            }
            log.plateau("embed_query_retained");
            log.reserve(6)?;
            let started = Instant::now();
            // In this frozen fixture every document exceeds 1500 characters, so worker's
            // padded 3000-character bound necessarily produces four singleton batches.
            let vectors = if args[0] == "bounded" {
                assert!(texts[..4].iter().all(|text| text.chars().count() > 1500));
                let mut vectors = Vec::new();
                for text in &texts[..4] {
                    vectors.extend(model.embed(&[*text])?);
                }
                vectors
            } else {
                model.embed(&texts[..4])?
            };
            log.emit(
                "timing",
                "embed_batch4",
                json!({"ms":started.elapsed().as_secs_f64()*1000.0,"vectors":vectors.len()}),
            );
            drop(vectors);
            log.plateau("embed_batch_retained");
            embed = Some(model);
        }
        if args[0] != "embed" {
            log.reserve(5)?;
            log.emit("marker", "rerank_load_begin", json!(null));
            let started = Instant::now();
            let mut rerank = BuiltinReranker::bge_reranker_v2_m3(models, accelerator)?;
            if args.iter().any(|a| a == "--fp16-embed")
                && rerank.device() != bars_core::models::Device::DirectMl
            {
                return Err("FP16 pair requires actual DirectML rerank".into());
            }
            log.emit("timing", "rerank_load", json!({"ms":started.elapsed().as_secs_f64()*1000.0,"device":format!("{:?}",rerank.device())}));
            log.plateau("rerank_loaded");
            let mut reference = Vec::new();
            for n in 0..3 {
                let started = Instant::now();
                let scores = rerank.rerank(&fixture.query, &texts)?;
                log.emit(
                    "timing",
                    "rerank",
                    json!({"n":n,"ms":started.elapsed().as_secs_f64()*1000.0,"scores":scores}),
                );
                reference = scores;
            }
            log.plateau("rerank_retained");
            let started = Instant::now();
            drop(rerank);
            log.emit(
                "timing",
                "rerank_drop",
                json!({"ms":started.elapsed().as_secs_f64()*1000.0}),
            );
            log.plateau("rerank_dropped");
            log.reserve(5)?;
            let started = Instant::now();
            let mut reloaded = BuiltinReranker::bge_reranker_v2_m3(models, accelerator)?;
            log.emit(
                "timing",
                "rerank_reload",
                json!({"ms":started.elapsed().as_secs_f64()*1000.0}),
            );
            let started = Instant::now();
            let scores = reloaded.rerank(&fixture.query, &texts)?;
            if scores.iter().chain(&reference).any(|s| !s.is_finite()) {
                return Err("non-finite rerank scores".into());
            }
            let delta = scores
                .iter()
                .zip(&reference)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0_f32, f32::max);
            log.emit("timing", "rerank_after_reload", json!({"ms":started.elapsed().as_secs_f64()*1000.0,"scores":scores,"max_score_delta":delta}));
            if scores.len() != reference.len() || delta > 1e-5 {
                return Err("reload changes scores".into());
            }
            drop(reloaded);
            log.plateau("rerank_second_drop");
        }
        drop(embed);
        log.plateau("all_models_dropped");
        Ok(())
    }
}
#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    windows::run()
}
#[cfg(not(windows))]
fn main() {
    panic!("This memory spike requires Windows");
}
