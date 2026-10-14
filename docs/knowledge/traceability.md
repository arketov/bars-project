---
title: Трассировка требований bars — какой документ описывает каждое Т-N
---

# Трассировка требований bars

Для каждого требования из MAIN_IDEA.md — документы `docs/`, где описано, как bars его выполняет. Таблица нужна, чтобы при проектировании и реализации ни одно требование не потерялось. Сверено 14-10-2026.

## Требования Т-1…Т-26

| Т-N | О чём | Где описано |
|---|---|---|
| Т-1 | один воркер, трей | [architecture/worker.md](../architecture/worker.md), [architecture/components.md](../architecture/components.md) |
| Т-2 | stdio-прокси, запуск воркера | [architecture/ipc.md](../architecture/ipc.md) |
| Т-3 | Rust, Tauri, fastembed-rs | [architecture/stack.md](../architecture/stack.md) |
| Т-4 | Windows и macOS | [architecture/stack.md](../architecture/stack.md); проверяется спайком сборки |
| Т-5 | `.bars` проекта | [storage/files.md](../storage/files.md), [storage/schema.md](../storage/schema.md) |
| Т-6 | три пути регистрации | [architecture/projects.md](../architecture/projects.md), [agent-tools/cli.md](../agent-tools/cli.md) |
| Т-7 | корень документации, модули | [indexing/updates.md](../indexing/updates.md), [storage/settings.md](../storage/settings.md) |
| Т-8 | `BARS_INSTRUCTION.md` | [integrations/bars-instruction.md](../integrations/bars-instruction.md) |
| Т-9 | инкрементальная индексация | [indexing/updates.md](../indexing/updates.md) |
| Т-10 | четыре способа найти изменения | [indexing/updates.md](../indexing/updates.md), [architecture/worker.md](../architecture/worker.md) |
| Т-11 | `title` из frontmatter | [indexing/chunking.md](../indexing/chunking.md) |
| Т-12 | гибридный поиск | [search/hybrid-search.md](../search/hybrid-search.md) |
| Т-13 | состав результата | [search/hybrid-search.md](../search/hybrid-search.md) |
| Т-14 | выбор модулей и файлов | [search/hybrid-search.md](../search/hybrid-search.md); дерево в окне — дизайн окна, ещё не описан |
| Т-15 | проект сессии по cwd | [architecture/protocol.md](../architecture/protocol.md), [architecture/projects.md](../architecture/projects.md) |
| Т-16 | настройки моделей и чисел | [storage/settings.md](../storage/settings.md) |
| Т-17 | три способа подключить модель | [models/local-models.md](../models/local-models.md), [models/external-models.md](../models/external-models.md) |
| Т-18 | значения по умолчанию, переопределение агентом | [storage/settings.md](../storage/settings.md), [search/hybrid-search.md](../search/hybrid-search.md) |
| Т-19 | инструменты агента | [agent-tools/mcp-tools.md](../agent-tools/mcp-tools.md) |
| Т-20 | CLI | [agent-tools/cli.md](../agent-tools/cli.md), [architecture/protocol.md](../architecture/protocol.md) |
| Т-21 | установка в клиенты | [integrations/clients.md](../integrations/clients.md) |
| Т-22 | удалённые документы | [indexing/updates.md](../indexing/updates.md) |
| Т-23 | нарезка | [indexing/chunking.md](../indexing/chunking.md), [models/local-models.md](../models/local-models.md) (длина входа) |
| Т-24 | порог релевантности | [search/hybrid-search.md](../search/hybrid-search.md), [storage/settings.md](../storage/settings.md) |
| Т-25 | смена модели и согласие | [models/local-models.md](../models/local-models.md), [architecture/protocol.md](../architecture/protocol.md) |
| Т-26 | структура документа | [agent-tools/mcp-tools.md](../agent-tools/mcp-tools.md), [agent-tools/cli.md](../agent-tools/cli.md) |

## Нефункциональные требования

| НФТ | Где описано |
|---|---|
| НФТ-1 | [indexing/chunking.md](../indexing/chunking.md), [indexing/updates.md](../indexing/updates.md), [storage/schema.md](../storage/schema.md) |
| НФТ-2 | [architecture/vector-search.md](../architecture/vector-search.md); подтверждено на Windows — [spike-search.md](spike-search.md) |
| НФТ-3 | [architecture/components.md](../architecture/components.md), [architecture/worker.md](../architecture/worker.md) |

## Что ещё не описано

- Окно: экраны, дерево с галочками (Т-14), настройки (Т-16), предупреждение Т-25, установка (Т-21). Делается отдельным проходом дизайна.
