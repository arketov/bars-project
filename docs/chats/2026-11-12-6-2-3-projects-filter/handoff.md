---
title: Сессия 12-11-2026 — шаг 6.2.3, фильтр проектов
---

# Шаг 6.2.3 — фильтр проектов

Предыдущий handoff по теме: [автопоиск подпроектов](../2026-11-12-6-2-2-subprojects-discovery/handoff.md). Контракт — [subprojects.md](../../features/projects/subprojects.md), разделы «Метод `projects`» и «bars_status: фильтр проектов». Код написан без проверки сборкой: сборки, `cargo`, тесты и `check.ps1` не запускались; сигнатуры и вызывающие проверены чтением и поиском по репозиторию.

## Сделано

- Протокол: `ProjectsParams.filter`; `ProjectInfo.parent_name` (составное имя предка для `subproject_of`, когда предка нет в выдаче); `ProjectsOutput.registry_total` (только в ответе на `filter`, нужен пустому результату). Новый `crates/bars-protocol/src/project_filter.rs`: `filter_text` и `project_matches_filter` — единственное правило совпадения (подстрока, Unicode lowercase, `\` = `/`), тест с таблицей, общей с `matchProject` окна.
- Воркер: `Handler::projects` — `filter` вместе с `target` даёт `invalid_params` («filter searches the whole registry; drop project»), пустой `filter` — как без него; новый `handler/project_filter.rs` (`parents_first`, `matching`: предок раньше подпроектов, остальное — порядок реестра; единственное место отбора записей — шов для worktree). Порядок «предок раньше» применён и к полному списку. Подсказка об имени без префикса (`errors.rs`) называет `bars_status(filter="...")`.
- MCP: `bars_status(filter)` с английским описанием параметра и инструмента; сочетания: `filter`+`project` и `filter`+`refresh` — `invalid_params`, `filter`+`all` — как один `filter`, пустой — как без него; ответ — PROJECT с совпавшими, строка «per-project parts need one project…», VERSIONS (без INDEX, AUDITS, NEW TOPICS, GRAPH). `render.rs`: строки `subproject_of`, `origin`, `also_named`, `subprojects:` (`имя (состояние[, excluded from this index])`), `modules: none (not indexed yet)` у ленивого, `projects_matching` с пустым результатом «no project matches `text`; the registry has N projects…». `filter` добавлен в список полей истории вызовов (`observation.rs`).
- CLI: `bars-cli projects [--filter ТЕКСТ]` (`conflicts_with = "project"`), строки «подпроект» и «подпроекты» в человеческом виде, пустой результат с размером реестра.
- Тесты: протокол (общая таблица), воркер (`project_filter.rs`: порядок, подпроект без предка, юникод, цикл; `handler/tests.rs`: фильтр, пробелы, `\`, пустой, размер реестра, `target`+`filter`), MCP (`project_catalog.rs`: все сочетания и пустой результат; `render.rs`: блок подпроекта и пустой результат), CLI (`cli.rs`: фильтр, JSON, пустой, `--project` + `--filter` не разбирается).
- Документы: `subprojects.md` (статус, `parent_name`, `registry_total`, порядок, правила строк), `protocol.md`, `mcp-tools.md`, `agent-layer-project.md`, `cli.md`, `manual/projects.md`, `manual/agents.md`, обе копии `BARS_INSTRUCTION.md` (docs и ассеты ядра) и skill `bars-docs` (строка `bars_status`).

## Не сделано

- Совпадение по worktree и пометка `matched_by: worktree` — шаги 6.3.x; в `render.rs` и `handler/project_filter.rs` оставлены комментарии-швы.
- Окно (`ui/`) не тронуто: общая таблица случаев уже есть в `project-switcher.test.ts`, Rust-копия повторяет её.
- Ничего не собиралось и не запускалось: возможны ошибки компиляции, `clippy` и форматирования (`cargo fmt`: часть длинных `assert!` в тестах).

## Сложности

- Спека пишет, что части INDEX и GRAPH не выводятся «как при `all`», но текущий `all=true` их выводит для проекта сессии; `all` не менял, `filter` следует спеке.
- Пустой результат требует размера реестра, а `projects` его не отдавал: добавлено поле `registry_total`, а не второй вызов без фильтра.
- Подсказка `bars_status(filter=...)` после имени есть только в тексте без префикса главного; перечень подпроектов главного (6.2.1) уже её содержал.
- Порядок «предок раньше подпроектов» теперь и у полного списка: тесты, держащие `projects[0]`, затрагиваются только если подпроект зарегистрирован раньше предка.

## Дальше

- Шаг 6.3.1 (worktree) подключает совпадение по worktree в `project_filter::matching` и строки владельца в `project_block`.
- Первым делом при разрешённой сборке — `cargo fmt`, `clippy` и тесты `bars-protocol`, `bars-worker` (`handler`), `bars-mcp` (`project_catalog`, `render`), `bars-cli`.
