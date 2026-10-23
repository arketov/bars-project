---
title: Передача — очередь документов и пауза перед embed (3.2.1)
---

# Передача — очередь документов и пауза перед embed (3.2.1)

## Сделано

- Проект и отличия от него — [очередь документов](../../features/indexing/document-queue.md). Запись на документ, пауза Т-41 после правки, приоритет правки Т-42, пропуск ждущих документов проверкой, явная команда без паузы, удаление без паузы, записи не создаются в `reindex_required`.
- Ключ `[indexing] edit_pause_seconds` — [настройки](../../features/settings/settings.md). `activity.documents` и `index_retry` — [протокол](../../architecture/protocol.md); раздел «Документы» — [страница индексации](../../features/window/indexing-progress.md); [руководство](../../manual/activity.md) и [устройство воркера](../../architecture/worker.md) поправлены.
- Код: `crates/bars-worker/src/worker/doc_queue.rs`, `mod.rs`, `control.rs`, `queue.rs`; `crates/bars-core/src/indexing/indexer.rs`, `settings/`; `crates/bars-protocol/src/methods.rs`; `crates/bars-app/src/commands.rs` (`document_write` сообщает воркеру); `ui/src/lib/IndexingPage.svelte`, `app.svelte.ts`, `types.ts`, `mock.ts`.
- Тесты: `bars-worker --lib` 258 ок, `bars-core` (пропуск документа, настройки) ок, `bars-protocol` ок, `bars-cli` и `bars-mcp` ок, UI 432 ок. Сборка — через `vcvars64`, `CMAKE_GENERATOR=Ninja`, `CARGO_TARGET_DIR=C:/bars-build`.

## Не сделано

- Приёмка в установленном окне и на настоящей модели; ошибка провайдера по документу не воспроизводилась (только юнит-тест состояний).
- Записи для документов, найденных проверкой (`periodic`/`explicit`); счётчик «ждут паузы» в трее; ключ паузы в окне настроек.
- ISS-13, ISS-17 — приёмка установленного окна.
- Замер умолчания 30 с — этап 3.2.2.

## Сложности

- Чужие незавершённые правки (`audits`, `tree_io`) временно ломали сборку воркера; в `bars-app` падает `tree_io_tests::t03_names_and_paths_are_checked` — не из этой сессии.
- Поздняя запись `waiting` от эха слежения после явной проверки (см. журнал).

## Дальше

3.2.2 — замер паузы на настоящем потоке правок; затем 3.3 (дерево по диску) использует `documents_edited` для операций с файлами.
