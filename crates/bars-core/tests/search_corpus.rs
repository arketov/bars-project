//! Поиск по корпусу `docs/` самого bars со встроенной bge-m3. Модель берётся только из уже
//! скачанной папки `~/.bars/models`; нет её — тест пропускается, ничего не скачивается.
//!
//! Идентификаторы в запросах выбраны так, что встречаются ровно в одном документе. Если в
//! документации они появятся в другом месте, тест нужно обновить вместе с ней.

mod common;

use std::path::Path;
use std::time::Instant;

use bars_core::indexing::chunking::ChunkingParams;
use bars_core::indexing::indexer::{IndexSettings, UpdateOutcome, update_index};
use bars_core::models::{Accelerator, BuiltinEmbedder};
use bars_core::search::{SearchRequest, SearchSettings, search};
use bars_core::storage::{Index, Journal};
use common::cached_models_dir;

#[test]
fn finds_identifiers_and_respects_the_module_filter() {
    let Some(models) = cached_models_dir() else {
        eprintln!("пропущено: bge-m3 нет в ~/.bars/models");
        return;
    };
    let mut model = BuiltinEmbedder::bge_m3(&models, Accelerator::Cpu).unwrap();
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = tempfile::tempdir().unwrap();
    let mut index = Index::open(&dir.path().join("index.sqlite"), Journal::Wal).unwrap();
    let settings = IndexSettings {
        chunking: ChunkingParams::default(),
        docs_root: "docs".into(),
    };
    let outcome = update_index(&project, &mut index, &settings, &mut model).unwrap();
    let UpdateOutcome::Updated(report) = outcome else {
        panic!("{outcome:?}");
    };
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    eprintln!(
        "проиндексировано документов: {}, чанков: {}",
        index.document_count().unwrap(),
        index.chunk_count().unwrap()
    );

    let ask = |model: &mut BuiltinEmbedder, request: &SearchRequest| {
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
        ("agent_min_score", "docs/storage/settings.md"),
        ("model_doc_prefix", "docs/storage/schema.md"),
    ] {
        let response = ask(&mut model, &query(identifier));
        assert_eq!(response.hits[0].path, expected, "{identifier}");
    }

    // Т-14: при отмеченном модуле результатов из других модулей нет.
    let mut storage_only = query("порог релевантности результатов поиска");
    storage_only.modules = vec!["storage".into()];
    let response = ask(&mut model, &storage_only);
    assert!(!response.hits.is_empty());
    assert!(response.hits.iter().all(|hit| hit.module == "storage"));

    // Смысловой запрос без слов из документа находит нужный раздел среди первых пяти.
    let response = ask(&mut model, &query("как отсеять нерелевантные результаты"));
    let top: Vec<&str> = response.hits.iter().map(|hit| hit.path.as_str()).collect();
    assert!(
        top.iter().any(|path| {
            matches!(
                *path,
                "docs/storage/settings.md" | "docs/search/hybrid-search.md"
            )
        }),
        "{top:?}"
    );
}
