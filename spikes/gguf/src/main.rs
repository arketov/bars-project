use llama_cpp_2::{
    context::params::{LlamaContextParams, LlamaPoolingType},
    llama_backend::LlamaBackend, llama_batch::LlamaBatch,
    model::{LlamaModel, params::LlamaModelParams},
};
use std::{num::NonZeroU32, time::Instant};
mod bench;
mod embed;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|x| x == "bench") {
        return bench::run(&args[1..]);
    }
    if args.len() != 2 || !["cpu", "gpu"].contains(&args[1].as_str()) {
        return Err("spike-gguf <model.gguf> <cpu|gpu>; JSON-массив текстов в stdin".into());
    }
    if args[1] == "gpu" && !cfg!(feature = "vulkan") {
        return Err("бинарник собран без Vulkan".into());
    }
    let texts: Vec<String> = serde_json::from_reader(std::io::stdin())?;
    let start = Instant::now();
    let backend = LlamaBackend::init()?;
    let model = LlamaModel::load_from_file(&backend, &args[0],
        &LlamaModelParams::default().with_n_gpu_layers(if args[1] == "gpu" { 1000 } else { 0 }))?;
    let mut ctx = model.new_context(&backend, LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(8192)).with_n_batch(8192).with_n_ubatch(512)
        .with_n_threads(8).with_n_threads_batch(8)
        .with_embeddings(true).with_pooling_type(LlamaPoolingType::Last))?;
    eprintln!("load_seconds={:.3}", start.elapsed().as_secs_f64());
    let mut batch = LlamaBatch::new(8192, 1);
    let mut out = std::io::stdout().lock();
    use std::io::Write;
    for text in texts {
        let started = Instant::now();
        let mut tokens = model.vocab().tokenize(text.as_bytes(), true, true);
        if tokens.len() > 8192 { return Err("вход длиннее 8192 токенов".into()); }
        if tokens.is_empty() { tokens = model.vocab().tokenize(b" ", true, true); }
        ctx.clear_kv_cache();
        batch.clear();
        batch.add_sequence(&tokens, 0, false)?;
        ctx.decode(&mut batch)?;
        let v = ctx.embeddings_seq_ith(0)?;
        let norm = v.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
        if norm <= 0.0 || !norm.is_finite() { return Err("некорректный вектор".into()); }
        let vector: Vec<f32> = v.iter().map(|x| (*x as f64 / norm) as f32).collect();
        writeln!(out, "{}", serde_json::json!({"tokens": tokens.len(), "seconds": started.elapsed().as_secs_f64(), "vector":vector}))?;
        out.flush()?;
    }
    Ok(())
}
