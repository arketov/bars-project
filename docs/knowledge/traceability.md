---
title: Трассировка требований bars — какой документ описывает каждое Т-N
---

# Трассировка требований bars

Для каждого требования из MAIN_IDEA.md — документы `docs/`, где описано, как bars его выполняет. Таблица нужна, чтобы при проектировании и реализации ни одно требование не потерялось. Сверено 14-10-2026.

## Требования Т-1…Т-28

| Т-N | О чём | Где описано |
|---|---|---|
| Т-1 | один воркер, трей | [architecture/worker.md](../architecture/worker.md), [architecture/components.md](../architecture/components.md) |
| Т-2 | stdio-прокси, запуск воркера | [architecture/ipc.md](../architecture/ipc.md) |
| Т-3 | Rust, Tauri, fastembed-rs | [architecture/stack.md](../architecture/stack.md) |
| Т-4 | Windows и macOS | [architecture/stack.md](../architecture/stack.md); проверяется спайком сборки |
| Т-5 | `.bars` проекта | [architecture/files.md](../architecture/files.md), [architecture/schema.md](../architecture/schema.md) |
| Т-6 | три пути регистрации | [features/projects/projects.md](../features/projects/projects.md), [features/agent-tools/cli.md](../features/agent-tools/cli.md) |
| Т-7 | корень документации, модули | [features/indexing/updates.md](../features/indexing/updates.md), [features/settings/settings.md](../features/settings/settings.md) |
| Т-8 | `BARS_INSTRUCTION.md`, совет о раскладке | [features/docs-convention/bars-instruction.md](../features/docs-convention/bars-instruction.md), [features/docs-convention/layout.md](../features/docs-convention/layout.md) |
| Т-9 | инкрементальная индексация | [features/indexing/updates.md](../features/indexing/updates.md) |
| Т-10 | четыре способа найти изменения | [features/indexing/updates.md](../features/indexing/updates.md), [architecture/worker.md](../architecture/worker.md) |
| Т-11 | `title` из frontmatter | [features/indexing/chunking.md](../features/indexing/chunking.md) |
| Т-12 | гибридный поиск | [features/search/hybrid-search.md](../features/search/hybrid-search.md) |
| Т-13 | состав результата | [features/search/hybrid-search.md](../features/search/hybrid-search.md) |
| Т-14 | выбор модулей и файлов | [features/search/hybrid-search.md](../features/search/hybrid-search.md); дерево в окне — дизайн окна, ещё не описан |
| Т-15 | проект сессии по cwd | [architecture/protocol.md](../architecture/protocol.md), [features/projects/projects.md](../features/projects/projects.md) |
| Т-16 | настройки моделей и чисел | [features/settings/settings.md](../features/settings/settings.md) |
| Т-17 | три способа подключить модель | [features/models/local-models.md](../features/models/local-models.md), [features/models/external-models.md](../features/models/external-models.md) |
| Т-18 | значения по умолчанию, переопределение агентом | [features/settings/settings.md](../features/settings/settings.md), [features/search/hybrid-search.md](../features/search/hybrid-search.md) |
| Т-19 | инструменты агента | [features/agent-tools/mcp-tools.md](../features/agent-tools/mcp-tools.md) |
| Т-20 | CLI | [features/agent-tools/cli.md](../features/agent-tools/cli.md), [architecture/protocol.md](../architecture/protocol.md) |
| Т-21 | установка в клиенты | [features/clients/clients.md](../features/clients/clients.md) |
| Т-22 | удалённые документы | [features/indexing/updates.md](../features/indexing/updates.md) |
| Т-23 | нарезка | [features/indexing/chunking.md](../features/indexing/chunking.md), [features/models/local-models.md](../features/models/local-models.md) (длина входа) |
| Т-24 | порог релевантности | [features/search/hybrid-search.md](../features/search/hybrid-search.md), [features/settings/settings.md](../features/settings/settings.md) |
| Т-25 | смена модели и согласие | [features/models/local-models.md](../features/models/local-models.md), [architecture/protocol.md](../architecture/protocol.md) |
| Т-26 | структура документа | [features/agent-tools/mcp-tools.md](../features/agent-tools/mcp-tools.md), [features/agent-tools/cli.md](../features/agent-tools/cli.md) |
| Т-27 | стартовая инструкция MCP | [features/docs-convention/delivery.md](../features/docs-convention/delivery.md) |
| Т-28 | skills bars | [features/docs-convention/delivery.md](../features/docs-convention/delivery.md) |

## Нефункциональные требования

| НФТ | Где описано |
|---|---|
| НФТ-1 | [features/indexing/chunking.md](../features/indexing/chunking.md), [features/indexing/updates.md](../features/indexing/updates.md), [architecture/schema.md](../architecture/schema.md) |
| НФТ-2 | [architecture/vector-search.md](../architecture/vector-search.md); подтверждено на Windows — [spike-search.md](spike-search.md) |
| НФТ-3 | [architecture/components.md](../architecture/components.md), [architecture/worker.md](../architecture/worker.md) |

## Что ещё не описано

- Окно: экраны, дерево с галочками (Т-14), настройки (Т-16), предупреждение Т-25, установка (Т-21). Делается отдельным проходом дизайна.
