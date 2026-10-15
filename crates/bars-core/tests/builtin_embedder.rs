//! Встроенная bge-m3 по-настоящему. Модель берётся только из уже скачанной папки
//! `~/.bars/models`; нет её — тесты пропускаются, ничего не скачивается.

mod common;

use std::fs;
use std::path::Path;

use bars_core::indexing::chunking::ChunkingParams;
use bars_core::indexing::indexer::{IndexSettings, UpdateOutcome, update_index};
use bars_core::models::{Accelerator, BGE_M3_DIM, BuiltinEmbedder, Device, Embedder};
use bars_core::storage::{Index, Journal};
use common::cached_models_dir;

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (norm(a) * norm(b))
}

#[test]
fn cpu_embeddings_follow_meaning_and_keep_order() {
    let Some(models) = cached_models_dir() else {
        eprintln!("пропущено: bge-m3 нет в ~/.bars/models");
        return;
    };
    let mut model = BuiltinEmbedder::bge_m3(&models, Accelerator::Cpu).unwrap();
    assert_eq!(model.device(), Device::Cpu);

    // Длины разные, чтобы сортировка по длине внутри embed поменяла порядок.
    let texts = [
        "Как настроить поиск по документации проекта и чем отличается полнотекстовый поиск от векторного",
        "настройка поиска",
        "рецепт борща со свёклой, капустой и сметаной для большой семьи на выходные дни",
    ];
    let vectors = model.embed(&texts).unwrap();

    assert_eq!(vectors.len(), 3);
    assert!(vectors.iter().all(|v| v.len() == BGE_M3_DIM));
    assert!(
        cosine(&vectors[0], &vectors[1]) > cosine(&vectors[0], &vectors[2]),
        "порядок векторов нарушен или модель не различает смысл"
    );
}

#[test]
fn indexes_a_small_project_with_bge_m3() {
    let Some(models) = cached_models_dir() else {
        eprintln!("пропущено: bge-m3 нет в ~/.bars/models");
        return;
    };
    let mut model = BuiltinEmbedder::bge_m3(&models, Accelerator::Cpu).unwrap();
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "docs/a.md",
        "# Поиск\n\nГибридный поиск объединяет полнотекстовый и векторный поиск.\n",
    );
    write(
        dir.path(),
        "docs/m/b.md",
        "# Борщ\n\nСвёкла, капуста и сметана.\n",
    );
    let mut index = Index::open(&dir.path().join("index.sqlite"), Journal::Wal).unwrap();
    let settings = IndexSettings {
        chunking: ChunkingParams::default(),
        docs_root: "docs".into(),
    };

    let outcome = update_index(dir.path(), &mut index, &settings, &mut model).unwrap();

    let UpdateOutcome::Updated(report) = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(report.added, 2);
    assert_eq!(index.vector_count().unwrap(), index.chunk_count().unwrap());
    assert_eq!(
        index.stored_fingerprint().unwrap().as_ref(),
        Some(model.fingerprint())
    );
    let chunk = index
        .conn()
        .query_row("SELECT embed_hash FROM chunks LIMIT 1", [], |row| {
            row.get::<_, String>(0)
        })
        .unwrap();
    let vector = index.vector(&chunk).unwrap().unwrap();
    assert_eq!(vector.len(), BGE_M3_DIM);
    let norm = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-4, "вектор не нормализован: {norm}");
}

#[test]
fn auto_accelerator_gives_the_same_vectors_as_cpu() {
    let Some(models) = cached_models_dir() else {
        eprintln!("пропущено: bge-m3 нет в ~/.bars/models");
        return;
    };
    let mut auto = BuiltinEmbedder::bge_m3(&models, Accelerator::Auto).unwrap();
    eprintln!("Auto выбрал устройство: {:?}", auto.device());
    let mut cpu = BuiltinEmbedder::bge_m3(&models, Accelerator::Cpu).unwrap();
    let text = ["Устройство для встроенных моделей: видеокарта или процессор"];

    let a = auto.embed(&text).unwrap();
    let c = cpu.embed(&text).unwrap();

    assert!(cosine(&a[0], &c[0]) > 0.999, "{}", cosine(&a[0], &c[0]));
}

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}
