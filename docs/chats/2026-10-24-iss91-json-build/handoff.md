# bars-cli --json как есть и сверка сборок в hello (ISS-91) — итог сессии 24-10-2026

## Сделано
- `bars-cli --json` печатает ответ воркера как есть: `Client::call_value` отдаёт исходный `Value`, `emit` печатает его, человеческий вывод берёт разобранный ответ. Контракт — [cli.md](../../features/agent-tools/cli.md).
- Сверка сборок в `hello`: `worker_build` в ответе, `BUILD_ID` (коммит из `crates/bars-protocol/build.rs`), `Client::build_warning`; предупреждение в stderr у `bars-cli` и `bars-mcp`. Контракт — [protocol.md](../../architecture/protocol.md), «Сверка сборок в hello».
- Тесты: два на `emit` в `bars-cli`, три на `build_mismatch`, проверка тишины при одной сборке в `bars-worker` (`ipc/mod.rs`). Правки build.md, mcp-tools.md, issues.md (ISS-91 в «Исправленные»). Полный `scripts/check.ps1` зелёный.

## Не сделано
- Живая проверка на смешанной установке (старый воркер + новый клиент) не выполнялась: установка не просилась.

## Трудности
- Первый прогон check.ps1 упал на неиспользуемом импорте `serde::Serialize`; убран.

## Дальше
- Блок 2, этап 2.1 плана.
