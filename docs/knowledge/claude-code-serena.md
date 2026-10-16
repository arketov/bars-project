---
title: Serena и bars в Claude Code — какие инструменты включены, хуки, почему агент их обходит, output style
---

# Serena и bars в Claude Code: настройка и нюансы

Заметка по ISS-08 (Claude Code игнорирует bars и serena). Состояние на 16-10-2026, Serena 2.0.0, Claude Code в десктопном приложении, Windows 11. Что проверено, а что нет — в конце каждого раздела.

## Глобальная настройка Serena на машине автора

- Конфиг — `~/.serena/serena_config.yml`. Он почти дефолтный: `excluded_tools: []`, бэкенд LSP, интерфейс `tools`, `base_modes`: `interactive`, `editing`, `no-memories`.
- Serena подключена пользователем с контекстом `claude-code-worktrees` и ключом `--project-from-cwd`. Контекст — копия штатного `claude-code` с `single_project: false`, чтобы остался `activate_project` (нужен для worktree).
- В реестре Serena около 50 проектов, больше половины из них worktree'ы. Они засоряют список; убирать нужно вручную (правка `projects:` или временное включение `remove_project`).
- Язык проекта задаёт `.serena/project.yml`. У bars подключён только `rust`; Markdown, YAML и SQL Serena не разбирает.

## Какие инструменты Serena включены и почему

**Исключены контекстом** (`excluded_tools`): `create_text_file`, `read_file`, `execute_shell_command`, `find_file`, `list_dir`, `search_for_pattern`. Все шесть дублируют встроенные Read, Write, Bash, Glob и Grep Claude Code.

**Исключены режимами:** память и онбординг (`no-memories`; память живёт в auto-memory и CLAUDE.md), `jet_brains_*` (нужен плагин JetBrains, у автора LSP), `insert_at_line`, `replace_lines`, `delete_lines` (дублируют Edit), `open_dashboard`, `serena_info`, `remove_project`.

**Добавлены автором через `included_optional_tools`:**

- `get_diagnostics_for_symbol` — ошибки LSP по одному символу, дешевле диагностики файла.
- `restart_language_server` — перезапуск зависшего или отставшего LSP без перезапуска сессии.
- `query_project` и `list_queryable_projects` — чтение кода другого зарегистрированного проекта без `activate_project`; нужны для межрепозиторных вопросов в firetech и tmc-opovechenie.
- `serena_repl` — Python-REPL поверх операций Serena (бета). Полный REPL-интерфейс (`agent_interface: REPL`) заменяет весь набор инструментов; он не включён.

Новые инструменты появляются только после перезапуска MCP-сервера Serena, то есть в новой сессии. Документация Serena не говорит, когда включать опциональные инструменты: список выше — выбор автора, а не рекомендация Serena.

## Хуки Serena в Claude Code

В `~/.claude/settings.json` стоят все четыре: `serena-hooks remind` (PreToolUse), `auto-approve` (PreToolUse для `mcp__serena__.*`), `activate` (SessionStart), `cleanup` (SessionEnd). Serena называет их «strongly recommended», сейчас они в альфе. Счётчики хуков лежат в `~/.serena/hook_data/<id сессии>/`.

`remind` слабый: он блокирует только третий подряд Grep или Read по коду без вызова Serena. Вызовы semble, bars, PowerShell и Bash счётчик обходят, а чтение файлов через `Get-Content` хук не видит вовсе.

## Почему Claude Code обходит Serena и bars

- Описания встроенных Read, Grep и Edit в системном промпте Claude Code длинные, пользователь их не меняет, и они сильно подталкивают модель к встроенным инструментам. Serena пишет, что без подмены промпта убедить модель ей пользоваться «почти невозможно».
- Рычаг, который рекомендует сама Serena, — `claude --system-prompt="$(serena prompts print-cc-system-prompt-override)"`. Команда рабочая (выдаёт около 150 строк), но флаг доступен только при запуске `claude` из терминала. **В десктопном приложении аргументы не передать**, системный промпт там задаёт приложение.
- Правило в CLAUDE.md слабее подмены промпта. Формулировка «semble находит, Serena работает» агентов не останавливала: они находили файл через semble и дальше читали его через Read. Теперь в `~/.claude/CLAUDE.md` сказано: semble — глобальная навигация и только указывает место, Serena — локальная навигация и работа с кодом в найденном файле.
- bars подталкивает агента только текстом инструкций MCP-сервера и хуком SessionStart. Хук `bars-cli hook session-start` ничего не пишет в stdout, а Claude Code откладывает MCP-инструменты, и видны только их имена. Serena пробивается текстом собственного хука, bars нет (запись журнала 16-10-2026 в `docs/chats/2026-10-16-plan-review/log.md`).

## Обходной путь для десктопа: output style

Output style — Markdown-файл в `~/.claude/output-styles/`, который Claude Code добавляет к системному промпту. Файл `~/.claude/output-styles/serena-bars.md` содержит сжатые правила выбора инструментов Serena, разделение semble и Serena и порядок работы с bars (поиск в документации до чтения файлов, `bars_outline` и `bars_read` вместо чтения целиком).

Что не проверено: показывает ли десктопное приложение пользовательские стили в списке и заметно ли агент стал чаще звать Serena и bars. Стиль нужно выбрать в приложении вручную, потом в новой сессии прогнать реальную задачу по коду и по документации и посмотреть, какие инструменты вызваны. Стиль лишь дополняет промпт: описания встроенных инструментов остаются.

Для bars из этого вытекает идея для ISS-08: тот же текст, что в output style, можно отдавать агенту через `BARS_INSTRUCTION.md` и инструкции MCP-сервера, а не рассчитывать на один SessionStart-хук.
