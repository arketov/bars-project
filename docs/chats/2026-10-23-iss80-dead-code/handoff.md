---
title: Передача — снято подавление dead_code с модуля воркера (ISS-80)
---

# Передача: снято подавление dead_code с модуля воркера (ISS-80)

## Сделано

- С `pub mod worker` в `crates/bars-worker/src/lib.rs` снят `#[allow(dead_code, unused_imports)]`. Компилятор нашёл три места, все — мёртвый код, окну из них ничего не нужно:
  - `JobQueue::len` (`worker/queue.rs`) удалён, тест считает задания через `snapshot().len()`;
  - `DocQueue::len` и `is_empty` (`worker/doc_queue.rs`): `is_empty` удалён, `len` оставлен только для тестов (`#[cfg(test)]`, не `pub`);
  - поле `ReindexReason` у `Outcome::ReindexRequired` (`worker/job.rs`) никто не читал — вариант стал без поля, две одинаковые ветки `match` объединены, места сопоставления в `worker/mod.rs` и `worker/operations.rs` обновлены.
- `pwsh -File scripts/check.ps1` зелёный целиком: fmt, clippy `-D warnings`, `cargo build`, `cargo test` (все прошли), `npm run check`, `npm test`.
- Решение автора 23-10-2026: кэш сборки один — `C:\bt\bars`, других каталогов и файлов вне проекта не создавать. Записано в `AGENTS.md` и [build.md](../../knowledge/build.md); `check.ps1`, `probe-*.ps1`, `run-memory-acceptance.ps1`, `measure-graph-process.ps1`, `test-client-profile-isolation.ps1`, `test-client-refresh.ps1`, `test-docs-delivery.ps1`, `test-mcp-disk-read.mjs` по умолчанию берут `C:/bt/bars`.
- ISS-80 перенесён в «Исправленные проблемы» в `docs/backlog/issues.md`.

## Не сделано

- Правки скриптов внесены после запуска `check.ps1`: проверен только синтаксис парсером PowerShell и `node --check`, сами probe- и test-скрипты не запускались.

## Сложности

- В `target\` репозитория llama.cpp падает с C1083 на длинном пути; `subst R:` не помог — Cargo разворачивает путь. Рабочий способ — `CARGO_TARGET_DIR=C:/bt/bars`.
- Сборка с `-p bars-worker` и с `--workspace` даёт две разные сборки llama.cpp (разное объединение фич); проверки запускать только с `--workspace`.
- `docking_probe` в `clippy --all-targets` требует `ui/docking-dist`: `npm run probe:docking:build` в `ui/`.

## Дальше

Пункт 1.1.3 — тест выгрузки модели (ISS-64); его незакоммиченная правка `worker/model_lifecycle.rs` и папка `chats/2026-10-23-iss64-unload-test/` чужие и в этот коммит не вошли.
