---
title: Исследование стека bars — сравнения, отклонённые варианты, риски
---

# Исследование стека bars

Исследование 14-10-2026, по которому выбран стек [architecture/stack.md](../architecture/stack.md). Здесь — что сравнивали, почему отклонили и какие риски проверяет прототип. Версии сверены по crates.io, npm и docs.rs на 14-10-2026.

## Версия fastembed-rs

Актуальная версия — 7.1.1 (07-10-2026). Цифра 5.17.2 в источниках MAIN_IDEA устарела: это релиз 15-06-2026. Что важно в ветке 7:

- `InitOptions` устарел, используется `TextInitOptions`;
- candle-модели (Qwen3, Nomic v2 MoE) кешируются в `FASTEMBED_CACHE_DIR`;
- `ort` закреплён на `=2.0.0-rc.13`;
- Qwen3 в README заявлен в размерах 0.6B, 4B и 8B.

Подводные камни fastembed-rs:

- `BGERerankerV2M3` скачивает сторонний ONNX-экспорт `rozgo/bge-reranker-v2-m3`, а не репозиторий BAAI.
- Переменная `HF_HOME` перекрывает и `FASTEMBED_CACHE_DIR`, и `with_cache_dir()`. Поэтому воркер должен сам задавать папку моделей и не зависеть от окружения пользователя.
- Своей фичи CoreML у fastembed нет. Доступность CoreML/DirectML определяется целевой платформой и сборкой ONNX Runtime; регистрация EP сама по себе не добавляет отсутствующую библиотеку и не доказывает размещение всей модели на ускорителе (OQ-27).

Источники: https://crates.io/api/v1/crates/fastembed, https://github.com/Anush008/fastembed-rs/releases, https://docs.rs/fastembed/7.1.1/fastembed/

## ONNX Runtime в сборке

По умолчанию `ort` при сборке скачивает статически линкуемый ONNX Runtime с `cdn.pyke.io`. Альтернатива — `load-dynamic`: dll или dylib рядом с exe, путь задаётся через `ORT_DYLIB_PATH`.

Выбрана статическая линковка: один файл и никаких поисков dll на машине пользователя.

Актуальное исследование ускорителей и альтернатив на 18-10-2026 — [inference-devices.md](inference-devices.md): в частности, Windows ML и отдельный OpenVINO Plugin EP. Это кандидаты; статическая линковка и зависимости bars этим обзором не меняются. Методы оптимизации действующего пути — [inference-speed.md](inference-speed.md).

Цена этого выбора:

- сборке нужна сеть;
- сборки для x86-64 требуют процессор уровня x86-64-v3;
- на Windows нужна VS 2022 не старше 17.14, иначе ошибки линковки `__std_*`.

Источники: https://ort.pyke.io/misc/prebuilt-binaries, https://ort.pyke.io/setup/linking, https://ort.pyke.io/troubleshooting/compiling

## Векторный поиск: sqlite-vec, свой перебор или ANN

| Вариант | За | Против |
|---|---|---|
| sqlite-vec 0.1.9 | векторы и фильтры по метаданным прямо в SQL | версия 0.1.x; открыт issue #21 — `cl.exe` не собирает `sqlite-vec.c` на Windows 11; синтаксис косинусной метрики в `vec0` не задокументирован |
| **свой перебор в Rust** | ни одной C-зависимости; фильтр по модулям — обычный SQL; скалярное произведение нормализованных векторов | код свой, но это десяток строк |
| usearch 2.26, ANN | быстро на миллионах векторов | лишняя сборка C++; на десятках тысяч векторов ANN не нужен |
| LanceDB 0.40 | FTS со стеммингом и векторы в одном месте | релиз раз в две недели, тяжёлое дерево Arrow; гибридный поиск для Rust не описан |
| turso 0.8 | векторы в самом SQL | версия 0.x, совместимость с FTS5 не подтверждена |

Выбран свой перебор. Сам sqlite-vec на таком объёме тоже перебирает все векторы: по замеру автора — меньше 75 мс на 100 000 векторов размерности 1024. Поэтому скорость у вариантов одинаковая, а свой перебор снимает риск сборки.

Источники: https://github.com/asg017/sqlite-vec/issues/21, https://alexgarcia.xyz/sqlite-vec/features/vec0.html, https://alexgarcia.xyz/blog/2024/sqlite-vec-stable-release/index.html

## Русская морфология в полнотекстовом поиске

Токенизатор FTS5 `unicode61` понимает кириллицу, но стеммер у FTS5 есть только английский (porter). Варианты:

| Вариант | Оценка |
|---|---|
| только префиксный поиск `слово*` | частично ловит окончания; «настройки» не найдёт «настройка» |
| **стемминг до FTS5 через `rust-stemmers`** | одна база; текст и запрос стеммятся одинаково. Крейт не обновлялся с 2019 года, но это алгоритмы Snowball, они стабильны |
| `trigram`-токенизатор | поиск по подстроке, но шумит и раздувает индекс |
| tantivy 0.26 | лучший полнотекстовый поиск, русский стеммер встроен; но это второе хранилище рядом с SQLite, и его нужно держать согласованным |

Выбран стемминг до FTS5. Если на корпусе bars качество окажется плохим, следующий кандидат — tantivy. Проверка 03-11-2026 на 168 запросах бенча: основной выигрыш даёт устройство полей и точная форма, а не смена движка или лемматизация; остаться на FTS5 рекомендовано, tantivy по документации, без замера — [токенизация по языкам](tokenization.md).

Источники: https://www.sqlite.org/fts5.html, https://crates.io/api/v1/crates/rust-stemmers, https://docs.rs/tantivy/0.26.2/tantivy/tokenizer/enum.Language.html

## Разбор Markdown и frontmatter

- **Markdown.** bars нужны только ATX-заголовки, ограждённые блоки кода и точные номера строк. Свой построчный разбор даёт номера строк даром. Готовые парсеры дают меньше и тяжелее:
  - `pulldown-cmark` отдаёт байтовые смещения и распознаёт setext-заголовки;
  - `comrak` строит полное дерево;
  - `markdown-rs` не обновлялся с 2025-04.

  `pulldown-cmark` 0.13 оставлен в тестах: на нём проверяется, что свой разбор находит те же ATX-заголовки.
- **YAML.** `serde_yaml` объявлен устаревшим. `serde_yml` брать нельзя: на него есть RUSTSEC-2025-0067 (не поддерживается) и RUSTSEC-2025-0068 (unsound), репозиторий заархивирован. `serde_norway` не обновлялся с 2024-12. Выбран `serde-saphyr` 2.0. Версия вышла 13-10-2026, поэтому её закрепляем точно и проверяем `cargo audit`.

Источники: https://docs.rs/pulldown-cmark/0.13.4/pulldown_cmark/, https://osv.dev/vulnerability/RUSTSEC-2025-0068

## Локальный канал и MCP

- **Канал.** `interprocess` 2.4 даёт один API над именованным каналом Windows и Unix-сокетом и поддерживает tokio. `parity-tokio-ipc` не обновлялся с 2021 года. `jsonrpsee` из коробки умеет только HTTP и WebSocket, а методов у bars мало, поэтому JSON-RPC свой, построчный.
- **Дескриптор канала Windows.** Дескриптор по умолчанию даёт группе Everyone право чтения. Поэтому DACL канала задаётся явно — только текущий пользователь ([architecture/ipc.md](../architecture/ipc.md)).
- **MCP.** `rmcp` 3.5 — официальный Rust SDK. Он реализует ревизию MCP 2026-07-28 и совместим с 2025-11-25. Переход на 3.x ломал API, поэтому версию закрепляем.

Источники: https://docs.rs/interprocess/latest/interprocess/local_socket/index.html, https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights, https://github.com/modelcontextprotocol/rust-sdk

## Tauri: упаковка и установка

- **Svelte без SvelteKit.** Шаблон Svelte в `create-tauri-app` сделан на SvelteKit. Проект без SvelteKit собирается так: `create-vite` с шаблоном `svelte-ts`, затем `tauri init`.
- **Куда кладутся `bars-mcp` и `bars-cli`.** Через `bundle.externalBin` они попадают рядом с `bars`:
  - macOS — `bars.app/Contents/MacOS/`;
  - NSIS — `$INSTDIR`, по умолчанию `%LOCALAPPDATA%\<ProductName>`;
  - MSI — `Program Files\<ProductName>`.
- **Абсолютные пути в конфигах клиентов.** Путь зависит от установщика, поэтому его строит сам bars от своего расположения и не хардкодит.
- **Известная проблема NSIS.** При переустановке sidecar может не замениться (issue #15134). Это касается OQ-17.
- **macOS.** Нотаризация требует hardened runtime на всех вложенных исполняемых файлах.

Источники: https://v2.tauri.app/learn/system-tray/, https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-bundler/src/bundle/macos/app.rs, https://github.com/tauri-apps/tauri/issues/15134

## Хуки и MCP в Claude Code и Codex

- **Claude Code `SessionStart`.**
  - Во входном JSON есть `cwd` и `source` (`startup`, `resume`, `clear`, `compact`, `fork`).
  - Хук описывается в `hooks.SessionStart` в `~/.claude/settings.json`.
- **Codex.**
  - Хуки уровня пользователя лежат в `~/.codex/hooks.json` или в таблицах `[hooks]` файла `~/.codex/config.toml`. Формат тот же, что у Claude Code, вход `SessionStart` тоже содержит `cwd`.
  - **Хук не запустится, пока пользователь не одобрит его командой `/hooks` в Codex.** Одобрение привязано к хешу записи: после любой правки нужно одобрять заново.
- **Codex `[mcp_servers.<имя>]`.**
  - Ключи stdio-сервера: `command`, `args`, `env`, `cwd`.
  - `startup_timeout_sec` по умолчанию 10 с. Поэтому `bars-mcp` отвечает на `initialize` сразу, не дожидаясь воркера.

Источники: https://code.claude.com/docs/en/hooks, https://learn.chatgpt.com/docs/hooks.md, https://learn.chatgpt.com/docs/extend/mcp?surface=cli

## Риски стека и запасные пути

Исход проверок на 19-10-2026 — в последнем столбце; macOS нигде не проверена (Т-4).

| Риск | Как проверить | Если не прошло | Исход |
|---|---|---|---|
| `ort` или `rusqlite` не собираются на Windows MSVC или macOS | спайк сборки на обеих ОС | `ort` с `load-dynamic` и dll рядом с exe | Windows собирается ([build.md](build.md)); macOS не проверена |
| Перебор и FTS5 медленнее НФТ-2 | замер на 5 000 и 50 000 чанках | держать векторы проекта в памяти воркера; затем ANN | Замер на Windows — [spike-search.md](spike-search.md) |
| Стемминг плохо ищет по-русски | сравнение на корпусе `docs/` bars | tantivy | Используется `rust-stemmers`; перехода на tantivy не было |
| Воркер, запущенный из `bars-mcp`, завершается вместе с сессией: Claude Code на Windows может убивать дерево процессов через job object | спайк в Claude Code и Codex на обеих ОС | `CREATE_BREAKAWAY_FROM_JOB`; иначе запуск воркера через автозапуск (OQ-16) | Воркер переживает сессию Claude Code на Windows; Codex и macOS не проверены ([spike-detach.md](spike-detach.md)) |
| `~/.claude.json` одновременно пишет Claude Code | правка при запущенном клиенте | перечитывать перед записью, писать атомарно через временный файл | Так и сделано ([clients.md](../features/clients/clients.md)) |
| Qwen3-Embedding-4B слишком медленный на CPU | замер индексации | предупреждение в окне уже предусмотрено; GPU — OQ-27 | Скорость и устройства — [inference-speed.md](inference-speed.md), [inference-devices.md](inference-devices.md) |
