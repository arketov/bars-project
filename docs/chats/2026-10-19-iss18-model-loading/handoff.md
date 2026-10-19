---
title: ISS-18 ложный model_loading — итог сессии 19-10-2026
---

# ISS-18 ложный model_loading — итог сессии 19-10-2026

## Сделано

- Воркер: `ModelSlot::is_loaded` и `RerankSlot::is_loaded` (`bars-worker/src/worker/models.rs`, функция `loaded`) больше не считают занятый мьютекс незагруженной моделью. При занятом замке готовность решает `SlotRuntime.state == "ready"`. Поиск во время индексации встаёт в очередь перед пачкой (`with_priority`), а не отвечает `model_loading` 100 %. То же для rerank: одновременный поиск больше не пропускает rerank.
- MCP: `call_observed` повторяет вызов на `model_loading` раз в секунду до `MODEL_WAIT` = 60 с (`Env::model_wait`), затем отдаёт ошибку с пояснением, что модель, скорее всего, ещё скачивается.
- Настройка «через сколько выгружать модель» для embed и rerank (просьба автора): ключ `unload_after_minutes` в `[embed]`/`[rerank]`, `0` — не выгружать, до 10 080 минут. `bars_core::settings::ModelUnload` в `UserSettings` (не в `EmbedSettings`/`RerankSettings`: смена срока не пересоздаёт слот), `ModelIdlePolicy::with_settings` в цикле выгрузки, поля `unload_after_minutes` в `EmbedDoc`/`RerankDoc`, запись в `settings_io`, блок «Выгрузка из памяти» на вкладке «Модели».
- Документы: `features/models/local-models.md`, `features/settings/settings.md`, `features/window/settings-window.md`, `manual/models.md`, `architecture/worker.md`, `features/agent-tools/mcp-tools.md`, `knowledge/build.md`, ISS-18 в `backlog/issues.md`, этап ISS-18 в плане.
- Тесты: `a_loaded_model_busy_with_a_batch_is_ready_not_loading` (со старым поведением падает с `Loading { percent: 0 }`), три теста `wait_for_model` в `bars-mcp`. `cargo test --release -p bars-worker -p bars-mcp`: worker lib 196 ok, mcp 5 + 12 ok; clippy `--lib --tests` без замечаний.

- Проверки после настройки выгрузки: `cargo test --release` bars-core 224, bars-protocol 53, bars-worker 199, bars-mcp 5 + 12 — ok; clippy без новых предупреждений (три прежних: chunking.rs, builtin.rs, `enum Call`); vitest 140 ok; блок «Выгрузка из памяти» проверен в dev-окне на mock: ошибка при 20000, сохранение 0.

## Не сделано

- В живом приложении не проверено: bars не пересобран и не переустановлен.
- 5 ГБ памяти при индексации не исследованы — ISS-18 остаётся открытым по этой части.
- Не закоммичено.

## Сложности

- На машине Shket сборки не было: поставлены через winget LLVM 23.1.3, Ninja, Vulkan SDK 1.4.363.0 (`C:\VulkanSDK\1.4.363.0`), CMake уже был. Окружение: Developer Shell VS 18 Community x64, `LIBCLANG_PATH=C:\Program Files\LLVM\bin`, `VULKAN_SDK`, `CMAKE_GENERATOR=Ninja`, `CMAKE_BUILD_PARALLEL_LEVEL=1`, `CARGO_TARGET_DIR=C:\bars-build`, `C:\Program Files\CMake\bin` в PATH.
- Каждая MCP-попытка при ожидании модели — отдельная запись истории MCP.

## Дальше

- Собрать и установить bars, проверить поиск окном и через MCP во время полной индексации.
- Память индексации — шаг ISS-18 в плане.
