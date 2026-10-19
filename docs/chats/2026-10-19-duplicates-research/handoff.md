# Передача: get_duplicates (шаги 16–17, IDEA-13)

## Сделано
- Исследование: [duplicates.md](../../knowledge/duplicates.md), данные — [duplicates-2026-10-19/](../../knowledge/duplicates-2026-10-19/Index.md), скрипт `scripts/duplicates-probe.mjs`.
- Реализация: ядро `crates/bars-core/src/search/duplicates.rs` (+ `duplicates/tests.rs`, 13 тестов), метод `duplicates` протокола, обработчик воркера, MCP `bars_get_duplicates` (project обязателен), CLI `bars-cli duplicates`, пример замера `examples/bench_duplicates.rs`.
- Документы: `maintenance-tools.md` (контракт и замер), `mcp-tools.md`, `cli.md`, `protocol.md`, manual, `bars-docs` SKILL, `ideas.md` (IDEA-13 закрыт), CHANGELOG, PLAN (этап убран, хвост проверок добавлен).
- Тесты: `cargo test --release` для bars-core, bars-protocol, bars-worker, bars-mcp, bars-cli — зелёные. Замер: 1 066 чанков bars, медиана 362 мс.

## Не сделано
- Живое приложение и настоящая модель, независимая разметка и подбор порогов, замер на тысячах чанков; ничего не закоммичено мной (чужой коммит 80b09b8 захватил ранний вариант ядра).
- Экран статистики MCP и окно про `bars_get_duplicates` не знают (чужие незакоммиченные правки UI/history в дереве).

## Сложности
- Сборка идёт из PowerShell с `CARGO_TARGET_DIR=C:/bars-build`, `CMAKE_GENERATOR=Ninja`, `VULKAN_SDK`; `clippy -D warnings` падает на уже существующих замечаниях bars-graph.

## Дальше
- Принять `bars_get_duplicates` в установленном приложении после переустановки; решить, нужен ли отдельный приёмочный набор с независимой разметкой.
