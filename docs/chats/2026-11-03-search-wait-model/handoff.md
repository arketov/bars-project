---
title: Шаг 2.2.1 — поиск ждёт загрузки моделей (ISS-105)
---

# Шаг 2.2.1 — поиск ждёт загрузки моделей (ISS-105)

Продолжение раунда 33 ([handoff раунда](../2026-11-03-search-pivot/handoff.md)). Код написан, но **не собирался и не проверялся**: автор запретил сборки и тесты («иначе комп умрёт»), rust-analyzer (serena) на Rust тоже не запускался. Код не закоммичен (`AGENTS.md`: коммит кода только после зелёного `scripts/check.ps1`).

## Сделано

- **Предел ожидания — 20 секунд на обе модели вместе** (`MODEL_WAIT` в `crates/bars-worker/src/handler/mod.rs`, настройки нет). Основание: в журналах сессий замера загрузка bge-m3 занимала 3,48 с, rerank 5,63 с ([search-quality.md](../../knowledge/search-quality.md)); `worker.jsonl` даёт только моменты завершения загрузки, длительностей в нём нет. Загрузки идут по очереди под одним замком, вместе около 9 с; 20 с — двукратный запас на холодный диск и инициализацию GPU, меньше срока вызова MCP (55 с). Настройкой не сделано: она тянет `settings.md`, схему, окно и миграции, а умолчание достаточно.
- **Поиск** ([hybrid-search.md](../../features/search/hybrid-search.md), «Ожидание моделей»): embed, скачанный и грузящийся в память (`percent = 100`), ждётся опросом каждые 50 мс без блокировки потока; rerank грузится параллельно с embed и ждётся тем же сроком, до замка проекта. Не скачанная модель (`percent < 100`), `Failed` и Т-25 — как раньше.
- **Embed не успел:** `search_with_signals(..., semantic = false)` в `bars-core` — только полнотекстовый список, косинуса и порога близости нет (`applied.min_score = 0`), расширение кандидатов графом пропускается (причина — `fallback_reason`), «Новые темы» такую выдачу не собирают. Новое поле ответа `semantic_error` (`bars-protocol`), строка `semantic_error:` в MCP и текстах графа, «Внимание:» в CLI, баннер и скрытая оценка в окне.
- **Rerank не успел:** выдача без rerank после ожидания, причина в `rerank_error`, как прежде.
- **Параллельные поиски** делят одну заявку на загрузку (`Loads::claim`), тесты проверяют ровно одну загрузку.
- **Документы:** hybrid-search, error-states, local-models, mcp-tools, protocol, manual (models, search, troubleshooting), ISS-105 (статус; в таблицу исправленных не перенесён — нет проверки).
- **Тесты (написаны, не запускались):** `crates/bars-core/src/search/hybrid/tests.rs` (поиск без смысловой части не зовёт embed, rerank без неё работает), `crates/bars-worker/src/handler/tests/model_wait.rs` (быстрее и медленнее предела, параллельные поиски, rerank быстрее и медленнее предела); в `FakeProvider` добавлены `files_on_disk` и `rerank_load_delay`.

## Не сделано

- Любая сборка, тест, `clippy`, `scripts/check.ps1`, проверка окна через preview.
- Тест ошибки загрузки при ожидании не добавлен: `a_failed_background_load_answers_provider_error_with_the_error_kind` покрывает `Failed` без ожидания.
- Баннер окна и скрытие оценки без проверки на экране; тестов окна нет (ни `ResultCard`, ни `SearchPane`).
- Строка про ожидание в `docs/BARS_INSTRUCTION.md` и skills агентов не правилась: файл занят чужой правкой.

## Сложности

- Запрет сборки: код правился через Read/Edit, форматирование — `rustfmt` без компиляции. Возможны ошибки типов и заимствований, особенно в `handler/mod.rs` (rerank-ожидание вынесено до замка проекта, `graph_budgets.filter(..)`) и `hybrid.rs`.

## Дальше

Код, ждущий общей проверки (`pwsh -File scripts/check.ps1`): `crates/bars-core/src/search/{hybrid.rs,mod.rs,hybrid/tests.rs}`, `crates/bars-protocol/src/{methods.rs,graph_text.rs}`, `crates/bars-worker/src/handler/{mod.rs,graph.rs,history.rs,tests.rs,tests/model_wait.rs}`, `crates/bars-worker/src/worker/models.rs`, `crates/bars-cli/src/render.rs`, `crates/bars-mcp/src/render.rs`, `ui/src/lib/{types.ts,SearchPane.svelte,ResultCard.svelte}`. Чужие незакоммиченные правки в тех же файлах (например `handler/mod.rs`, `bars-protocol/src/lib.rs`) смотреть в `git diff` перед коммитом. После зелёной проверки: коммит кода, ISS-105 в таблицу исправленных, шаг 2.2.1 из плана — по «да» автора.
