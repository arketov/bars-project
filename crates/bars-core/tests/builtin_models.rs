//! Встроенные rerank и candle-модели по-настоящему. Модели берутся только из уже скачанной папки
//! `~/.bars/models`; нет модели — проверка пропускается, ничего не скачивается.

mod common;

use bars_core::models::{
    Accelerator, BuiltinReranker, NOMIC_V2_MOE_ID, QWEN3_0_6B_ID, Reranker, builtin_model,
    load_builtin_embedder, prepare_environment,
};
use common::cached_model_dir;

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[test]
fn builtin_rerank_scores_are_probabilities_in_input_order() {
    let Some(models) = cached_model_dir("models--rozgo--bge-reranker-v2-m3", "model.onnx") else {
        eprintln!("пропущено: bge-reranker-v2-m3 нет в ~/.bars/models");
        return;
    };
    let mut reranker = BuiltinReranker::bge_reranker_v2_m3(&models, Accelerator::Cpu).unwrap();
    let documents = [
        "Свёкла, капуста и мясо варятся два часа: так готовят борщ.",
        "Очередь индексации берёт по одному проекту за раз.",
        "Погода в Москве завтра облачная.",
    ];
    let scores = reranker
        .rerank("как устроена очередь индексации", &documents)
        .unwrap();

    assert_eq!(scores.len(), 3);
    assert!(
        scores.iter().all(|score| (0.0..=1.0).contains(score)),
        "{scores:?}"
    );
    // Порядок оценок — порядок документов, а не порядок убывания.
    assert!(scores[1] > scores[0] && scores[1] > scores[2], "{scores:?}");
    assert!(scores[1] > 0.1 && scores[0] < 0.1, "{scores:?}");
    assert!(reranker.rerank("запрос", &[]).unwrap().is_empty());
}

/// Qwen3-Embedding-0.6B и Nomic Embed v2 MoE через candle; каждая проверка идёт, если модель
/// уже скачана.
fn check_candle_model(id: &str, repo: &str, marker: &str) {
    let Some(models) = cached_model_dir(repo, marker) else {
        eprintln!("пропущено: {id} нет в ~/.bars/models");
        return;
    };
    prepare_environment(&models);
    let info = builtin_model(id).unwrap();
    let mut model = load_builtin_embedder(id, &models, Accelerator::Cpu).unwrap();
    assert_eq!(model.fingerprint(), &info.fingerprint());

    let query = model
        .embed_query("как настроить поиск по документации")
        .unwrap();
    let documents = model
        .embed(&[
            "Поиск по документации настраивается в окне bars: можно выбрать модель и пороги.",
            "рецепт борща со свёклой, капустой и сметаной для большой семьи",
        ])
        .unwrap();
    assert_eq!(query.len(), info.dim);
    assert!(documents.iter().all(|vector| vector.len() == info.dim));
    assert!(
        cosine(&query, &documents[0]) > cosine(&query, &documents[1]),
        "{id}: модель не различает смысл"
    );
}

#[test]
fn qwen3_embedding_through_candle() {
    check_candle_model(
        QWEN3_0_6B_ID,
        "models--Qwen--Qwen3-Embedding-0.6B",
        "model.safetensors",
    );
}

#[test]
fn nomic_embed_v2_moe_through_candle() {
    check_candle_model(
        NOMIC_V2_MOE_ID,
        "models--nomic-ai--nomic-embed-text-v2-moe",
        "model.safetensors",
    );
}
