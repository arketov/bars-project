---
title: Документация bars
---

# Документация bars

bars — локальный поиск по Markdown-документации проектов для человека и агента: окно, CLI и MCP используют общий индекс проекта. bars возвращает исходные фрагменты с путями и строками; ответ LLM не генерирует.

## Где искать ответ о bars

| Вопрос | Документ |
|---|---|
| Что такое bars, кому нужен, что делает | [Обзор проекта](overview.md) |
| Как установить, добавить проект, найти документ, подключить агента | [Первый запуск](getting-started.md) |
| Что реализовано, что ещё не проверено | [Состояние проекта](project-status.md) |
| Как ищет, что означает оценка | [Гибридный поиск](features/search/hybrid-search.md) |
| Почему изменения ещё не видны | [Обновление индекса](features/indexing/updates.md) |
| Какие команды и инструменты доступны | [CLI и MCP](features/agent-tools/Index.md) |
| Где данные и настройки | [Файлы bars](architecture/files.md), [настройки](features/settings/settings.md) |
| Как писать Markdown для индексации | [Соглашение о документации](features/docs-convention/Index.md), [инструкция bars](BARS_INSTRUCTION.md) |

## Источники правды документации bars

Требования Т-N и НФТ живут в [MAIN_IDEA.md](MAIN_IDEA.md); раздел «Решение» в нём ведёт к архитектурным документам. Текущие интерфейсы и устройство описаны в `features/` и `architecture/`. Исследования в `knowledge/` и история в `chats/` не подтверждают внедрение предложений. Открытые дефекты — в [issues.md](backlog/issues.md), пожелания — в [ideas.md](backlog/ideas.md).

Раскладка документации — по [соглашению](features/docs-convention/layout.md). Для RAG каждый тематический раздел называет предмет и читается отдельно от соседей.

## Содержание

- [overview.md](overview.md) — назначение bars, возможности, данные и модели.
- [getting-started.md](getting-started.md) — установка, регистрация, поиск, MCP и проверка индекса.
- [project-status.md](project-status.md) — реализованные части, ограничения и открытая приёмка.
- [BARS_INSTRUCTION.md](BARS_INSTRUCTION.md) — генерируемая bars инструкция по оформлению; в поисковый индекс не входит.
- [MAIN_IDEA.md](MAIN_IDEA.md) — требования всего проекта: исходная формулировка, Т-N, НФТ, уроки образца, критерии приёмки, журнал раундов.
- [features/](features/Index.md) — фичи bars: что умеет и как это работает; поставка (установщик, автозапуск) — [features/distribution/](features/distribution/Index.md).
- [architecture/](architecture/Index.md) — общее устройство: стек, процессы, канал, протокол, схема индекса.
- [adr/](adr/Index.md) — принятые решения с обоснованием.
- [knowledge/](knowledge/Index.md) — внутренняя база знаний: исследования, сравнения, замеры спайков.
- [chats/](chats/Index.md) — журнал сессий агентов: ход работы и передача дел.
- [backlog/](backlog/Index.md) — баги ISS-NN и пожелания IDEA-NN с состоянием и ссылками на проверки.
