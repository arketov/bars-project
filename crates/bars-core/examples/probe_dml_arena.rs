//! ISS-18: isolate DirectML arena with the same graph, shapes and synthetic tokens.
#[cfg(windows)]
#[allow(dead_code)]
#[path = "bench_memory.rs"]
mod memory;

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use memory::windows::{Log, Monitor};
    use ort::{
        session::{Session, builder::GraphOptimizationLevel},
        value::Tensor,
    };
    use serde_json::json;
    let a: Vec<_> = std::env::args().skip(1).collect();
    if a.len() != 3 || !matches!(a[0].as_str(), "arena" | "no-arena") {
        return Err("probe_dml_arena <arena|no-arena> <model.onnx> <new-jsonl>".into());
    }
    let log = Log::create(std::path::Path::new(&a[2]))?;
    log.reserve(8)?;
    let monitor = Monitor::start(log.clone());
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        log.plateau("baseline");
        let mut model = Session::builder()?
            .with_config_entry(
                "ep.dml.disable_memory_arena",
                if a[0] == "no-arena" { "1" } else { "0" },
            )?
            .with_memory_pattern(false)?
            .with_parallel_execution(false)?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_intra_threads(std::thread::available_parallelism()?.get())?
            .with_execution_providers([ort::ep::DirectML::default()
                .with_device_id(0)
                .build()
                .error_on_failure()])?
            .commit_from_file(&a[1])?;
        log.plateau("loaded");
        for round in 0..2 {
            for len in [32usize, 512, 1536, 2048, 32] {
                log.reserve(2)?;
                let ids = Tensor::from_array(([1usize, len], vec![42i64; len]))?;
                let mask = Tensor::from_array(([1usize, len], vec![1i64; len]))?;
                let started = std::time::Instant::now();
                let outputs = model.run(ort::inputs!["input_ids"=>ids,"attention_mask"=>mask])?;
                let (_, v) = outputs[0].try_extract_tensor::<f32>()?;
                let hash =
                    blake3::hash(&v.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>());
                log.emit("timing","run",json!({"round":round,"len":len,"ms":started.elapsed().as_millis(),"output_hash":hash.to_hex().to_string()}));
                drop(outputs);
                log.plateau("run_done");
            }
        }
        drop(model);
        log.plateau("dropped");
        Ok(())
    })();
    let result = result.and(monitor.stop());
    log.emit(
        "end",
        "end",
        json!({"ok":result.is_ok(),"error":result.as_ref().err().map(ToString::to_string)}),
    );
    result
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Windows-only experiment");
}
