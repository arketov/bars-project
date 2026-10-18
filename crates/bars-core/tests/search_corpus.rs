//! Поиск по корпусу `docs/` самого bars со встроенной bge-m3. Модель берётся только из уже
//! скачанной папки `~/.bars/models`; нет её — тест пропускается, ничего не скачивается.
//!
//! Идентификаторы в запросах выбраны так, что встречаются ровно в одном документе. Если в
//! документации они появятся в другом месте, тест нужно обновить вместе с ней.

mod common;

use std::path::Path;
use std::time::Instant;

use bars_core::indexing::chunking::ChunkingParams;
use bars_core::indexing::indexer::{
    IndexObserver, IndexSettings, UpdateOutcome, update_index_observed,
};
use bars_core::models::{
    Accelerator, BuiltinEmbedder, Device, EmbedError, Embedder, ModelFingerprint,
};
use bars_core::search::{SearchRequest, SearchSettings, search};
use bars_core::storage::{Index, Journal};
use common::cached_models_dir;

#[test]
#[ignore = "полная переиндексация: отдельный GPU-прогон с --ignored --nocapture"]
fn finds_identifiers_and_respects_the_module_filter() {
    let Some(models) = cached_models_dir() else {
        eprintln!("пропущено: bge-m3 нет в ~/.bars/models");
        return;
    };
    let cpu = std::env::var("BARS_CORPUS_TEST_DEVICE").as_deref() == Ok("cpu");
    let inner = BuiltinEmbedder::bge_m3(
        &models,
        if cpu {
            Accelerator::Cpu
        } else {
            Accelerator::Auto
        },
    )
    .unwrap();
    assert!(
        cpu || inner.device() == Device::DirectMl,
        "GPU недоступен; полный корпус на CPU автоматически не запускается"
    );
    eprintln!(
        "устройство: {:?}; пачка: 1; бюджет индексации: 60 с",
        inner.device()
    );
    let mut model = CorpusEmbedder(inner);
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = tempfile::tempdir().unwrap();
    let mut index = Index::open(&dir.path().join("index.sqlite"), Journal::Wal).unwrap();
    let settings = IndexSettings {
        chunking: ChunkingParams::default(),
        docs_root: "docs".into(),
    };
    let outcome = update_index_observed(
        &project,
        &mut index,
        &settings,
        &mut model,
        &CorpusBudget(Instant::now()),
    )
    .expect("корпус не завершён: ошибка или исчерпан бюджет 60 с");
    let UpdateOutcome::Updated(report) = outcome else {
        panic!("{outcome:?}");
    };
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    eprintln!(
        "проиндексировано документов: {}, чанков: {}",
        index.document_count().unwrap(),
        index.chunk_count().unwrap()
    );

    let ask = |model: &mut CorpusEmbedder, request: &SearchRequest| {
        let started = Instant::now();
        let response = search(&index, model, None, &SearchSettings::default(), request).unwrap();
        eprintln!(
            "«{}»: {} мс, первый — {:?}",
            request.query,
            started.elapsed().as_millis(),
            response.hits.first().map(|hit| (&hit.path, hit.score)),
        );
        response
    };
    let query = |text: &str| SearchRequest {
        query: text.into(),
        ..SearchRequest::default()
    };

    // Т-12: точный идентификатор, которого нет в соседних документах, — первым без реранка.
    for (identifier, expected) in [
        ("agent_min_score", "docs/features/settings/settings.md"),
        ("model_doc_prefix", "docs/architecture/schema.md"),
    ] {
        let response = ask(&mut model, &query(identifier));
        assert_eq!(response.hits[0].path, expected, "{identifier}");
    }

    // Т-14: при отмеченном модуле результатов из других модулей нет.
    let mut features_only = query("порог релевантности результатов поиска");
    features_only.modules = vec!["features".into()];
    let response = ask(&mut model, &features_only);
    assert!(!response.hits.is_empty());
    assert!(response.hits.iter().all(|hit| hit.module == "features"));

    // Смысловой запрос без слов из документа находит нужный раздел среди первых пяти.
    let response = ask(&mut model, &query("как отсеять нерелевантные результаты"));
    let top: Vec<&str> = response.hits.iter().map(|hit| hit.path.as_str()).collect();
    assert!(
        top.iter().any(|path| {
            matches!(
                *path,
                "docs/features/settings/settings.md" | "docs/features/search/hybrid-search.md"
            )
        }),
        "{top:?}"
    );
}

// Тест не должен занимать все CPU или запускать большие GPU-пачки без отдельного решения.
struct CorpusEmbedder(BuiltinEmbedder);
impl Embedder for CorpusEmbedder {
    fn fingerprint(&self) -> &ModelFingerprint {
        self.0.fingerprint()
    }
    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let mut result = Vec::with_capacity(texts.len());
        for text in texts {
            result.extend(self.0.embed(&[*text])?);
        }
        Ok(result)
    }
    fn embed_query(&mut self, text: &str) -> Result<Vec<f32>, EmbedError> {
        self.0.embed_query(text)
    }
}
struct CorpusBudget(Instant);
impl IndexObserver for CorpusBudget {
    fn should_stop(&self) -> bool {
        self.0.elapsed() >= std::time::Duration::from_secs(60)
    }
}
