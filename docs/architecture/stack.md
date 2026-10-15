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
- C-компилятор нужен в любом случае: SQLite (`rusqlite` bundled), `blake3` и `aws-lc-rs` в `reqwest` собираются из C.

## Процесс, окно и трей bars

| Что | Крейт / пакет | Версия | Зачем |
|---|---|---|---|
| Окно и трей | `tauri` с фичей `tray-icon` | 2.12 | Т-1, Т-3 |
| Один экземпляр воркера | `tauri-plugin-single-instance` | 2.5 | второй запуск `bars` показывает окно работающего (Т-1) |
| Интерфейс окна | `svelte` + `vite` + TypeScript | 5.57 / 8.3 | без SvelteKit; проект создаётся `create-vite` (шаблон `svelte-ts`) и `tauri init` |
| Async-рантайм | `tokio` | 1.53 | воркер, клиенты канала |

Воркер не завершается при закрытии последнего окна: `RunEvent::ExitRequested` с `code == None` отменяется через `prevent_exit()`. На macOS у воркера нет иконки в Dock: `ActivationPolicy::Accessory`.

`tauri-plugin-autostart` и `tauri-plugin-updater` подключаются, когда будут решены OQ-16 и OQ-17. `tauri-plugin-shell` не нужен: воркер сам никого не запускает. Логи пишет `tracing`, а не `tauri-plugin-log`, чтобы у воркера, `bars-mcp` и `bars-cli` был один механизм.

## Модели bars

| Что | Крейт | Версия | Зачем |
|---|---|---|---|
| Встроенные embed и rerank | `fastembed` с фичами `qwen3`, `nomic-v2-moe`, `directml` | 7.1.1 | Т-17; подробности — [features/models/local-models.md](../features/models/local-models.md) |
| ONNX Runtime | `ort` (через fastembed) | 2.0.0-rc.13 | статически вшивается в `bars` при сборке |
| HTTP к внешним моделям | `reqwest` с фичей `json` | 0.13.5 | Т-17; rustls по умолчанию |

Сборка проверена на Windows спайком: 30 МБ exe, Rust 1.99, Visual Studio 2026 ([knowledge/spike-search.md](../knowledge/spike-search.md)). ONNX Runtime скачивается при сборке с `cdn.pyke.io` и линкуется в `bars` статически: отдельной dll или dylib рядом с exe нет. Сборке нужен доступ к сети. Готовые сборки ONNX Runtime для x86-64 требуют процессор уровня x86-64-v3 (Intel Haswell, 2013, и новее).

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
| Разбор Markdown | свой построчный разбор | — | [features/indexing/chunking.md](../features/indexing/chunking.md); `pulldown-cmark` 0.13 — только в тестах как эталон |
| Frontmatter | `serde-saphyr` | 2.0 | `title` документа (Т-11); чистый Rust |
| Слежение за файлами | `notify` + `notify-debouncer-full` | 8.2 / 0.7 | Т-10 |
| Пути пользователя | `dirs` | 7.0 | `<домашняя папка>/.bars/` |
| Чтение `settings.toml` и `.bars/config.toml` | `toml` + `serde` | 1.1 / 1.0 | [features/settings/settings.md](../features/settings/settings.md); только чтение, запись из окна — `toml_edit` ниже |

## Агенты и клиенты bars

| Что | Крейт | Версия | Зачем |
|---|---|---|---|
| MCP-сервер | `rmcp` с фичами `server`, `transport-io`, `macros` | 3.5 | `bars-mcp`, Т-2, Т-19 |
| Локальный канал | `interprocess` с фичей `tokio` | 2.4 | [ipc.md](ipc.md) |
| Windows: DACL канала и владелец сервера | `windows-sys` (`bars-app` и `bars-protocol`, `cfg(windows)`) + `widestring` (`bars-app`) | 0.61 / 1.2 | SDDL для `interprocess`, SID пользователя, сверка владельца процесса сервера (OQ-38); версия `windows-sys` совпадает с той, что тянет `interprocess` |
| Протокол канала | `serde_json`, свой JSON-RPC 2.0 построчно | 1.0 | методов мало, подписки не нужны |
| CLI | `clap` с фичей `derive` | 4.6 | `bars-cli`, Т-20 |
| Ключи провайдеров | `keyring-core` + store-крейты Windows и Apple из семейства `keyring` 4.2 | 4.x | [secrets.md](secrets.md) |
| Правка `config.toml` Codex | `toml_edit` | 0.25 | сохраняет комментарии и порядок (Т-21) |
| Правка `~/.claude/settings.json` | `jsonc-parser` (CST) | 0.34 | сохраняет форматирование файла, который правят руками |
| Правка `~/.claude.json` | `serde_json` с фичей `preserve_order` | 1.0 | файл машинный; важнее атомарная запись, чем отступы |

## Общие крейты bars

| Что | Крейт | Версия |
|---|---|---|
| Логи | `tracing`, `tracing-subscriber`, `tracing-appender` | 0.1 / 0.3 / 0.2 |
| Ошибки | `thiserror` в библиотеках, `anyhow` в exe | 2.0 / 1.0 |

`tracing-appender` пишет файл на день и хранит последние 14 файлов (`max_log_files`).

## Что проверяется на прототипе

Проверено на Windows ([knowledge/spike-search.md](../knowledge/spike-search.md)): сборка, скорость поиска (НФТ-2), качество стемминга. Воркер, запущенный из `bars-mcp`, переживает конец сессии Claude Code ([knowledge/spike-detach.md](../knowledge/spike-detach.md)). Осталось: сборка и CoreML на macOS, Codex.
