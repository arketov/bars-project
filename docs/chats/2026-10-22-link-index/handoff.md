---
title: Сессия 22-10-2026 — индекс ссылок, пункт 3.5.1
---

# Сессия 22-10-2026 — индекс ссылок, пункт 3.5.1

## Done

- Проект и итог — в [индексе ссылок](../../features/knowledge-base/link-index.md); контракт `links_of` — [протокол](../../architecture/protocol.md).
- Общий разбор ссылок `crates/bars-graph/src/links.rs` (`parse_links`, `DocSet::resolve`); граф берёт из него сбор ссылок, `METHOD_VERSION` не менялась, 18 + 19 тестов `bars-graph` зелёные.
- Хранилище `crates/bars-core/src/link_index/` (`.bars/links.sqlite3`): 8 тестов + замер на 10 000 документов (сборка 1,5 с, сверка 44 мс, `links_of` 261 мс, отладочная сборка).
- Метод `links_of` (`crates/bars-protocol/src/links.rs`, `crates/bars-worker/src/handler/links.rs`), тест воркера.
- Окно: `ui/src/lib/document-links.ts`, панель «Ссылки» в `WorkspaceDocument.svelte` (входящие, исходящие, ссылка в никуда), мок `links_of`; `backlinks.ts` удалён; 405 тестов vitest зелёные.

## Not done

- Нативная приёмка панели в установленном окне; проверка в браузере с моком не выполнялась.
- Общий JSON-набор для Rust и vitest (L-01), `links_broken`, `links_plan_move`, правка ссылок при переименовании (3.5.2), переход по ссылке в никуда с созданием файла.

## Difficulties

- Сборка в каталоге с кириллицей падает; помогает `CARGO_TARGET_DIR=C:/bars-build`.
- Упал `ui/src/lib/graph/zz-bench.test.ts` (файл другой сессии, не связан с ссылками).

## Next

- 3.5.2: план правки ссылок поверх `DocSet` и `links_of`; затем `links_broken` для аудитов 3.8.3.
