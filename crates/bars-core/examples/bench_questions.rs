//! Исследование IDEA-14: векторы коротких вопросов для калибровки порога дедупликации.
//! `bench_questions <texts.json> <vectors.json> <auto|cpu|fp16-auto|fp16-cpu>`.
//! Каждый текст считается отдельным вызовом `embed_query`, как поисковый запрос; векторы
//! нормализуются здесь (индекс нормализует их сам, модель — нет).

use std::path::PathBuf;

use bars_core::models::{Accelerator, BuiltinEmbedder, Embedder};

fn main() {
    let mut args = std::env::args().skip(1);
    let texts_path = args.next().expect("texts.json");
    let out_path = args.next().expect("vectors.json");
    let mode = args.next().unwrap_or_else(|| "auto".into());

    let texts: Vec<String> = serde_json::from_slice(&std::fs::read(&texts_path).unwrap()).unwrap();
    let models = PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(".bars/models");
    let (fp16, accelerator) = match mode.as_str() {
        "auto" => (false, Accelerator::Auto),
        "cpu" => (false, Accelerator::Cpu),
        "fp16-auto" => (true, Accelerator::Auto),
        "fp16-cpu" => (true, Accelerator::Cpu),
        other => panic!("режим: auto | cpu | fp16-auto | fp16-cpu, а не {other}"),
    };
    let mut embedder = if fp16 {
        BuiltinEmbedder::bge_m3_fp16(&models, accelerator).unwrap()
    } else {
        BuiltinEmbedder::bge_m3(&models, accelerator).unwrap()
    };
    let fingerprint = format!("{:?}", embedder.fingerprint());
    eprintln!("{mode}: {} текстов; {fingerprint}", texts.len());

    let started = std::time::Instant::now();
    let mut vectors = Vec::with_capacity(texts.len());
    for text in &texts {
        let mut vector = embedder.embed_query(text).unwrap();
        let norm = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        vector.iter_mut().for_each(|x| *x /= norm);
        vectors.push(vector);
    }
    let seconds = started.elapsed().as_secs_f64();
    let result = serde_json::json!({
        "mode": mode,
        "fingerprint": fingerprint,
        "execution": format!("{:?}", embedder.execution()),
        "seconds": seconds,
        "vectors": vectors,
    });
    std::fs::write(out_path, serde_json::to_vec(&result).unwrap()).unwrap();
    eprintln!("готово за {seconds:.2} с");
}
