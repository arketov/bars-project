# Передача дел: «Новые темы», внутренний API окна (шаг 20, P3)

## Сделано
- Схема БД тем 2: таблицы `checks` и `check_results`, миграция с сохранением тем (`crates/bars-core/src/new_topics/store.rs`); методы `get`, `set_status`, `rename`, `delete_checked`, `record_check` с ожидаемой ревизией; 5 новых тестов ядра (22 в модуле).
- Протокол: девять методов только для окна `new_topics_*` (`crates/bars-protocol/src/new_topics.rs`, `methods.rs`); обработчик — `crates/bars-worker/src/handler/topics.rs`; обёртки сбора — `crates/bars-worker/src/new_topics.rs`; 6 новых тестов воркера (A4, A7, A8, A10, A12, ручная отметка).
- Документы: [design.md](../../features/new-topics/design.md#внутренний-api-окна), [plan.md](../../features/new-topics/plan.md), [requirements.md](../../features/new-topics/requirements.md), [protocol.md](../../architecture/protocol.md), `CHANGELOG.md`, OQ-102 в `QUESTIONS.md`, PLAN.md (шаг 20 сужен до P4).
- Проверено: `cargo test -p bars-protocol -p bars-core -p bars-worker --lib` (53, 264, 218 тестов), `cargo check` bars-app/cli/mcp.

## Не сделано
- Ничего не закоммичено; в рабочей копии есть чужая правка `docs/backlog/ideas.md`.
- P4: экран, строка дерева, «Проверить все» с отменой в окне, приёмка A1…A14 на настоящей bge-m3 и в Tauri, release-замер записи.
- Метка индекса для `conclusive` — `updated_at` с точностью до секунды; отдельного токена обновления нет.

## Сложности
- Сборка идёт только с коротким `CARGO_TARGET_DIR` (см. log.md).

## Дальше
- Шаг 20 плана, P4: экран «Новые темы» по [макету](../../features/new-topics/design/mockup.html) поверх методов из protocol.md. Ручная отметка берёт `request_id` отображённого поиска: окно пока передаёт в `_window` только `purpose: "diagnostic"` (`ui/src/lib/app.svelte.ts`), для обычного поиска `request_id` придётся завести.
