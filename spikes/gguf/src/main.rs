use llama_cpp_2::{
    context::params::{LlamaContextParams, LlamaPoolingType},
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{LlamaModel, params::LlamaModelParams},
};
use std::{num::NonZeroU32, time::Instant};
mod bench;
mod embed;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|x| x == "product") {
        return product(&args[1..]);
    }
    if args.first().is_some_and(|x| x == "hash") {
        use std::io::Read;
        for file in &args[1..] {
            let mut input = std::fs::File::open(file)?;
            let mut hasher = blake3::Hasher::new();
            let mut buffer = [0; 65536];
            loop {
                let n = input.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                hasher.update(&buffer[..n]);
            }
            println!("{} {}", hasher.finalize().to_hex(), file);
        }
        return Ok(());
    }
    if args.first().is_some_and(|x| x == "bench") {
        return bench::run(&args[1..]);
    }
    let coexist = args.first().is_some_and(|s| s == "coexist");
    let args = if coexist { &args[1..] } else { &args[..] };
    if (args.len() != 2 && !(coexist && args.len() == 3))
        || !["cpu", "gpu"].contains(&args[1].as_str())
    {
        return Err("spike-gguf <model.gguf> <cpu|gpu>; JSON-массив текстов в stdin".into());
    }
    let texts: Vec<String> = if coexist {
        vec!["поиск документации".into(), "поиск документации".into()]
    } else {
        serde_json::from_reader(std::io::stdin())?
    };
    let start = Instant::now();
    let backend = LlamaBackend::init()?;
    if args[1] == "gpu" && !backend.supports_gpu_offload() {
        return Err("Vulkan-устройство недоступно".into());
    }
    let model = LlamaModel::load_from_file(
        &backend,
        &args[0],
        &LlamaModelParams::default().with_n_gpu_layers(if args[1] == "gpu" { 1000 } else { 0 }),
    )?;
    let mut ctx = model.new_context(
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
    eprintln!("load_seconds={:.3}", start.elapsed().as_secs_f64());
    let mut batch = LlamaBatch::new(8192, 1);
    let mut out = std::io::stdout().lock();
    use std::io::Write;
    for (i, text) in texts.into_iter().enumerate() {
        if coexist && i == 1 {
            use bars_core::models::{
                Accelerator, BuiltinEmbedder, BuiltinReranker, Embedder, Reranker,
            };
            let mut embed =
                BuiltinEmbedder::bge_m3(std::path::Path::new(&args[2]), Accelerator::Auto)?;
            let mut rr = BuiltinReranker::bge_reranker_v2_m3(
                std::path::Path::new(&args[2]),
                Accelerator::Auto,
            )?;
            let bge = embed.embed_query("поиск документации")?;
            let scores = rr.rerank(
                "поиск документации",
                &[
                    "Документация проекта и поиск по ней",
                    "Настройка гоночной машины",
                ],
            )?;
            if bge.len() != 1024
                || !bge.iter().all(|x| x.is_finite())
                || scores.len() != 2
                || !scores.iter().all(|x| x.is_finite())
            {
                return Err("ORT: некорректный ответ".into());
            }
            eprintln!(
                "ORT embed={:?}; rerank={:?}; scores={scores:?}",
                embed.execution(),
                rr.execution()
            );
        }
        let started = Instant::now();
        let mut tokens = model.vocab().tokenize(text.as_bytes(), true, true);
        if tokens.len() > 8192 {
            return Err("вход длиннее 8192 токенов".into());
        }
        if tokens.is_empty() {
            tokens = model.vocab().tokenize(b" ", true, true);
        }
        ctx.clear_kv_cache();
        batch.clear();
        batch.add_sequence(&tokens, 0, false)?;
        ctx.decode(&mut batch)?;
        let v = ctx.embeddings_seq_ith(0)?;
        let norm = v.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
        if norm <= 0.0 || !norm.is_finite() {
            return Err("некорректный вектор".into());
        }
        let vector: Vec<f32> = v.iter().map(|x| (*x as f64 / norm) as f32).collect();
        writeln!(
            out,
            "{}",
            serde_json::json!({"tokens": tokens.len(), "seconds": started.elapsed().as_secs_f64(), "vector":vector})
        )?;
        out.flush()?;
    }
    Ok(())
}

fn product(a: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bars_core::models::{Accelerator, BuiltinReranker, Reranker, load_builtin_embedder};
    use std::io::Write;
    if (a.len() != 3 && a.len() != 4) || !["cpu", "gpu"].contains(&a[2].as_str()) {
        return Err("product <model-id> <models-dir> <cpu|gpu>".into());
    }
    let texts: Vec<String> = serde_json::from_reader(std::io::stdin())?;
    let mut model = load_builtin_embedder(
        &a[0],
        std::path::Path::new(&a[1]),
        if a[2] == "cpu" {
            Accelerator::Cpu
        } else {
            Accelerator::Auto
        },
    )?;
    let mut reranker = None;
    for (i, text) in texts.into_iter().enumerate() {
        if a.len() == 4 && i == 1 {
            let mut rr = BuiltinReranker::bge_reranker_v2_m3(
                std::path::Path::new(&a[3]),
                Accelerator::Auto,
            )?;
            let scores = rr.rerank(
                "поиск документации",
                &["Поиск по документации проекта", "Настройки гоночной машины"],
            )?;
            if scores.len() != 2 || !scores.iter().all(|s| s.is_finite()) {
                return Err("ORT: некорректные оценки".into());
            }
            eprintln!("ORT coexist: {:?}; scores={scores:?}", rr.execution());
            reranker = Some(rr);
        }
        let start = Instant::now();
        let vector = model.embed(&[&text])?.remove(0);
        let execution = model.execution();
        if a[2] == "gpu" && execution.actual != Some("vulkan") {
            return Err(format!("Vulkan не подтверждён: {execution:?}").into());
        }
        println!(
            "{}",
            serde_json::json!({"vector":vector,"seconds":start.elapsed().as_secs_f64(),"actual":execution.actual,"backend":execution.backend,"precision":execution.precision,"fallback":execution.fallback_reason,"fingerprint":model.fingerprint().model})
        );
        std::io::stdout().flush()?;
    }
    drop(reranker);
    Ok(())
}
