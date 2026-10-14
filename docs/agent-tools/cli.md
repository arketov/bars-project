---
title: Команды bars-cli
---

# Команды bars-cli

`bars-cli` — отдельный exe для людей, хуков и агентов, которые тестируют приложение (Т-20). Каждая команда — то же действие, что в окне или в MCP-инструменте, и даёт тот же результат.

## Общие правила bars-cli

- Проект по умолчанию — ближайшая к текущей папке зарегистрированная папка; `--project <путь>` задаёт его явно.
- Вывод по умолчанию — для человека; `--json` — тот же ответ, что у MCP-инструмента.
- Код выхода: `0` — успех, `1` — ошибка в запросе, `2` — воркер недоступен, `3` — нужна переиндексация (Т-25). Соответствие ошибкам канала — [architecture/protocol.md](../architecture/protocol.md).

## Список команд bars-cli

| Команда | Что делает | Соответствие |
|---|---|---|
| `bars-cli search <запрос> [--module M]… [--file F]… [--limit N] [--min-score S]` | поиск | `bars_search` |
| `bars-cli outline <путь> [--levels 2,3] [--under "A/B" \| --under-line N]` | структура документа | `bars_outline` (Т-26) |
| `bars-cli read <путь>` | документ целиком | `bars_read` |
| `bars-cli projects` | проекты и модули | `bars_projects` |
| `bars-cli status` | состояние индекса | `bars_status` |
| `bars-cli reindex [--full] [--accept-model-change]` | переиндексация; `--accept-model-change` — согласие пользователя после смены модели (Т-25, [architecture/protocol.md](../architecture/protocol.md)) | `bars_reindex` |
| `bars-cli register [путь]` | регистрация проекта (Т-6) | пункт «Добавить проект» в окне |
| `bars-cli unregister` | снять проект с регистрации ([architecture/projects.md](../architecture/projects.md)) | кнопка «Убрать из bars» в окне |
| `bars-cli install --client claude-code\|codex` | подключить bars к клиенту (Т-21) | пункт «Установить» в окне |
| `bars-cli uninstall --client claude-code\|codex` | убрать bars из клиента | пункт «Удалить» в окне |
| `bars-cli hook session-start` | команда для хука начала сессии | — |

## Хук начала сессии

`bars-cli hook session-start` читает из stdin JSON, который передаёт клиент, и берёт из него рабочую папку сессии — поле `cwd`; у Claude Code и Codex формат входа одинаковый. Затем находит корень проекта, регистрирует его, если он ещё не зарегистрирован и в нём есть документация ([architecture/projects.md](../architecture/projects.md)), и ставит проект в очередь проверки актуальности (Т-6, Т-10). Команда не ждёт индексации и завершается сразу, чтобы не задерживать старт сессии. Ошибки она пишет в лог воркера и всегда выходит с кодом `0`: сбой bars не должен ломать сессию агента.
