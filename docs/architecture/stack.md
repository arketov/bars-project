---
title: Стек bars — библиотеки, версии и почему они выбраны
---

# Стек bars

Основа задана требованием Т-3: Rust, Tauri, fastembed-rs, без Python. Остальное выбрал агент 14-10-2026 по исследованию; сравнения, отклонённые варианты и источники — в [knowledge/stack-research.md](../knowledge/stack-research.md). Версии — актуальные на 14-10-2026; в `Cargo.toml` они закрепляются точно там, где крейт ещё 0.x или недавно ломал API.

## Инструменты сборки bars

- **Rust** stable не старше 1.90 (этого требует `tauri-plugin-single-instance`; `rusqlite` требует 1.88). Версия закрепляется в `rust-toolchain.toml`.
- **Windows:** MSVC из Visual Studio 2022 не старше 17.14 — более старые не линкуют ONNX Runtime из `ort`.
- **macOS:** Xcode Command Line Tools; сборки arm64 и x86_64.
- **Node.js** LTS — только для сборки интерфейса окна.
- C-компилятор нужен в любом случае: SQLite (`rusqlite` bundled), `blake3` и часть зависимостей Tauri собираются из C.

## Процесс, окно и трей bars

| Что | Крейт / пакет | Версия | Зачем |
|---|---|---|---|
| Окно и трей | `tauri` с фичей `tray-icon` | 2.12 | Т-1, Т-3 |
| Один экземпляр воркера | `tauri-plugin-single-instance` | 2.5 | второй запуск `bars` показывает окно работающего (Т-1) |
| Интерфейс окна | `svelte` + `vite` + TypeScript | 5.57 / 8.3 | без SvelteKit; проект создаётся `create-vite` (шаблон `svelte-ts`) и `tauri init` |
| Карта графа в окне | `cytoscape` | 3.34.4 | просмотр графа документов; выбор после сравнения с Sigma — [renderer-проверка](../knowledge/graph-renderer-2026-10-18/Index.md#выбор-renderer-для-p3) |
| Системные диалоги и открытие | `tauri-plugin-dialog`, `tauri-plugin-opener` | 2 | диалоги ОС и открытие файлов/ссылок из окна |
| Async-рантайм | `tokio` | 1.53 | воркер, клиенты канала |

Воркер не завершается при закрытии последнего окна: `RunEvent::ExitRequested` с `code == None` отменяется через `prevent_exit()`. На macOS у воркера нет иконки в Dock: `ActivationPolicy::Accessory`.

`tauri-plugin-autostart` не нужен: автозапуск — свой небольшой модуль воркера, который пишет то же значение реестра, а воркер собирается без Tauri ([features/distribution/autostart.md](../features/distribution/autostart.md)). `tauri-plugin-updater` не нужен: обновление идёт запуском новой версии установщика, без подписанных релизов и сервера обновлений ([features/distribution/installer.md](../features/distribution/installer.md)). `tauri-plugin-shell` не нужен: воркер сам никого не запускает. Файловая диагностика использует фасад `log` 0.4 и writer с ограничением размера и ротацией ([worker.md](worker.md), [контракт логов](../features/window/logs.md)); сторонние модули отфильтрованы.

## Модели bars

| Что | Крейт | Версия | Зачем |
|---|---|---|---|
| Встроенные ONNX embed/rerank и Nomic | `fastembed` с фичами `nomic-v2-moe`, Windows `directml` | 7.1.1 | Т-17; Qwen использует llama.cpp по ADR-0002, подробности — [features/models/local-models.md](../features/models/local-models.md) |
| ONNX Runtime | `ort` (через fastembed) | 2.0.0-rc.13 | статически вшивается в `bars` при сборке |
| HTTP к внешним моделям | `ureq` с фичей `json` | 3.4 | Т-17; блокирующий клиент под синхронные trait'ы `Embedder` и `Reranker`, rustls; уже есть в дереве зависимостей через fastembed (решение агента вместо `reqwest`, OQ-59) |
| candle для Nomic | `candle-core` | 0.11 | `Device` и `DType` для загрузчиков fastembed; версия та же, что у fastembed |

Сборка проверена на Windows спайком: 30 МБ exe, Rust 1.99, Visual Studio 2026 ([knowledge/spike-search.md](../knowledge/spike-search.md)). ONNX Runtime скачивается при сборке с `cdn.pyke.io` и линкуется в `bars` статически: отдельной dll или dylib рядом с exe нет. Сборке нужен доступ к сети. Готовые сборки ONNX Runtime для x86-64 требуют процессор уровня x86-64-v3 (Intel Haswell, 2013, и новее).

Qwen GGUF Q4_K_M использует закреплённый llama-cpp-2 0.1.160 / llama.cpp по [ADR-0002](../adr/0002-qwen-gguf.md); владение контекстом — self_cell 1.3.0, SHA-256 — sha2 0.10. На Windows backend Vulkan, на macOS Metal (не проверена); CPU fallback с теми же весами. Сборка Windows: Vulkan SDK, Ninja, MSVC 17.14+, короткий target. Подписанный Vulkan loader с лицензией включается в установщик; SDK у пользователя не требуется.

## Индекс bars

| Что | Крейт | Версия | Зачем |
|---|---|---|---|
| SQLite | `rusqlite` с фичей `bundled` | 0.40.2 | Т-5; встроенный SQLite 3.53.2 уже собран с FTS5 |
| Полнотекстовый поиск | FTS5 из SQLite | — | Т-12 |
| Русская морфология для FTS5 | `rust-stemmers` | 1.2.0 | стемминг русского и английского до записи в FTS5 |
| Векторный поиск | свой точный перебор в Rust | — | [vector-search.md](vector-search.md) |
| Хеши файлов и чанков | `blake3` | 1.8 | НФТ-1, Т-9 |

Расширение sqlite-vec не используется: на объёме одного проекта оно ничего не даёт сверх перебора, а сборка под Windows MSVC у него не подтверждена. Почему — в [vector-search.md](vector-search.md).

## Документы и файлы bars

| Что | Крейт | Версия | Зачем |
|---|---|---|---|
| Разбор Markdown | свой построчный разбор | — | [features/indexing/chunking.md](../features/indexing/chunking.md): нарезка на чанки |
| Ссылки и исходные диапазоны Markdown | `pulldown-cmark` (в `bars-graph`, без HTML-рендерера) | 0.13.4 | граф документов ([graph.md](graph.md)); в нарезку чанков не входит |
| Клиентская схема входа графа | `schemars` (опционально, фича `schema` у `bars-graph`) | 1.2.3 | JSON-схема входа инструментов `bars-mcp` |
| Frontmatter | свой разбор (`split_frontmatter` в нарезке) | — | `title` документа (Т-11); YAML-библиотека не подключена |
| Слежение за файлами | `notify` + `notify-debouncer-full` | 8.2 / 0.7 | Т-10 |
| Пути пользователя | `dirs` | 7.0 | `<домашняя папка>/.bars/` |
| Чтение `settings.toml` и `.bars/config.toml` | `toml` + `serde` | 1.1 / 1.0 | [features/settings/settings.md](../features/settings/settings.md); только чтение, запись из окна — `toml_edit` ниже |

## Агенты и клиенты bars

| Что | Крейт | Версия | Зачем |
|---|---|---|---|
| MCP-сервер | `rmcp` с фичами `server`, `transport-io`, `macros` | 3.5 | `bars-mcp`, Т-2, Т-19 |
| Локальный канал | `interprocess` с фичей `tokio` | 2.4 | [ipc.md](ipc.md) |
| Windows: DACL канала, владелец сервера, запись автозапуска в реестре | `windows-sys` (`bars-worker` и `bars-protocol`, `cfg(windows)`) + `widestring` (`bars-worker`) | 0.61 / 1.2 | SDDL для `interprocess`, SID пользователя, сверка владельца процесса сервера (OQ-38); версия `windows-sys` совпадает с той, что тянет `interprocess` |
| Протокол канала | `serde_json`, свой JSON-RPC 2.0 построчно | 1.0 | методов мало, подписки не нужны |
| CLI | `clap` с фичей `derive` | 4.6 | `bars-cli`, Т-20 |
| Ключи провайдеров | `keyring-core` 1.0 + `windows-native-keyring-store` 1.1 и `apple-native-keyring-store` 1.0 (фича `keychain`) | 1.x | [secrets.md](secrets.md) |
| Правка `config.toml` Codex | `toml_edit` | 0.25 | сохраняет комментарии и порядок (Т-21) |
| Правка `~/.claude/settings.json` и `~/.claude.json` | `serde_json` с фичей `preserve_order` | 1.0 | порядок ключей сохраняется, комментарии в `settings.json` нет; файлы клиентов правятся атомарной записью (`bars-worker/src/clients`) |

## Общие крейты bars

| Что | Крейт | Версия |
|---|---|---|
| Ошибки | `thiserror` в библиотеках, `anyhow` в exe | 2.0 / 1.0 |

Файловый журнал с уровнями и ротацией реализован через `log` и writer воркера; контракт и границы — [logs.md](../features/window/logs.md).

## Что проверяется на прототипе

Проверено на Windows ([knowledge/spike-search.md](../knowledge/spike-search.md)): сборка, скорость поиска (НФТ-2), качество стемминга. Воркер, запущенный из `bars-mcp`, переживает конец сессии Claude Code ([knowledge/spike-detach.md](../knowledge/spike-detach.md)). Осталось: сборка и CoreML на macOS, Codex.
