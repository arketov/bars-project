//! Windows memory/lifecycle spike; independent process, no worker IPC or user settings.
#[cfg(windows)]
mod windows {
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
    struct Log {
        file: Arc<Mutex<File>>,
        start: Instant,
    }
    impl Log {
        fn emit(&self, kind: &str, phase: &str, detail: Value) {
            let row = json!({"kind":kind, "phase":phase, "elapsed_ms":self.start.elapsed().as_secs_f64()*1000.0,
                "unix_ms":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis(),
                "pid":std::process::id(), "memory":memory(), "detail":detail});
            let mut file = self.file.lock().unwrap();
            writeln!(file, "{row}").unwrap();
            file.flush().unwrap();
        }
        fn plateau(&self, phase: &str) {
            self.emit("marker", phase, json!(null));
            std::thread::sleep(Duration::from_secs(2));
            self.emit("plateau_end", phase, json!(null));
        }
        fn preflight(&self) -> Result<()> {
            if !headroom(&memory(), 4 * GIB, 8 * GIB) {
                self.emit(
                    "abort",
                    "preflight",
                    json!("RAM <4 GiB or commit headroom <8 GiB"),
                );
                return Err("insufficient headroom".into());
            }
            Ok(())
        }
    }
    #[derive(Deserialize)]
    struct Fixture {
        query: String,
        documents: Vec<String>,
    }
    pub fn run() -> Result<()> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.len() != 5
            || !matches!(args[0].as_str(), "embed" | "rerank" | "both")
            || !matches!(args[1].as_str(), "cpu" | "auto")
        {
            return Err("bench_memory <embed|rerank|both> <cpu|auto> <models> <fixture.json> <new-output.jsonl>".into());
        }
        // The fixture is frozen by the runner, and no settings/index/client files are opened.
        let fixture: Fixture = serde_json::from_slice(&std::fs::read(&args[3])?)?;
        if fixture.documents.len() != 15 {
            return Err("fixture must contain 15 documents".into());
        }
        let cache = Path::new(&args[2]);
        bars_core::models::prepare_environment(cache);
        for id in [
            bars_core::models::BGE_M3_ID,
            bars_core::models::BGE_RERANKER_V2_M3_ID,
        ] {
            if !bars_core::models::is_downloaded(cache, id) {
                return Err(format!("model is not fully cached, refusing download: {id}").into());
            }
        }
        let log = Log {
            file: Arc::new(Mutex::new(
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&args[4])?,
            )),
            start: Instant::now(),
        };
        log.preflight()?;
        log.emit("config", "startup", json!({"mode":args[0], "accelerator":args[1], "fixture_blake3":blake3::hash(&std::fs::read(&args[3])?).to_hex().to_string(), "interval_ms":100}));
        let done = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&done);
        let sampler_log = log.clone();
        let sampler = std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                if !headroom(&memory(), GIB / 2, GIB) {
                    sampler_log.emit(
                        "abort",
                        "low_memory",
                        json!("RAM <512 MiB or commit headroom <1 GiB"),
                    );
                    // This owned standalone process must stop before another large allocation.
                    std::process::exit(75);
                }
                sampler_log.emit("sample", "monitor", json!(null));
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        let result = exercise(&args, &fixture, &log);
        done.store(true, Ordering::Relaxed);
        sampler.join().unwrap();
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
            let mut model = BuiltinEmbedder::bge_m3(models, accelerator)?;
            log.emit("timing", "embed_load", json!({"ms":started.elapsed().as_secs_f64()*1000.0,"device":format!("{:?}",model.device())}));
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
            log.preflight()?;
            let started = Instant::now();
            let vectors = model.embed(&texts[..4])?;
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
            log.preflight()?;
            log.emit("marker", "rerank_load_begin", json!(null));
            let started = Instant::now();
            let mut rerank = BuiltinReranker::bge_reranker_v2_m3(models, accelerator)?;
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
            log.preflight()?;
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
