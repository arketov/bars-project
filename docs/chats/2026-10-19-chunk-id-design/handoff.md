---
title: Передача сессии — ID чанка и get_similar
---

# Передача сессии — ID чанка и get_similar

## Сделано

- Шаг 14: формат, срок действия, ошибки и видимость ID чанка — [maintenance-tools.md](../../features/search/maintenance-tools.md#идентификатор-чанка-для-get_similar); формат принят автором.
- Шаг 15 (код): `chunk_id` (`<номер>-<контроль>`) в выдаче поиска; ядро `search/similar.rs`; метод протокола `similar`, ошибка `chunk_not_found` (-32016); воркер, MCP `bars_get_similar`, CLI `bars-cli similar`. Тесты: ядро (6), воркер (сквозной через канал для CLI и MCP), протокол, MCP; все тесты затронутых крейтов проходят (`--release --lib --tests`).
- Документы: protocol, mcp-tools, cli, hybrid-search, maintenance-tools, search/Index, manual/agents, ideas (IDEA-12 отмечена).

## Не сделано

Приёмка на настоящей bge-m3 и в живом приложении, замер задержки, порог близости (по умолчанию его нет), статистика MCP для `bars_get_similar` (контракт покрывает шесть инструментов), правка стартовой инструкции MCP. Установленный bars старее репозитория: новый инструмент в нём появится после сборки и установки. Не коммитилось: в рабочей копии чужие правки.

## Сложности

Сборка из Git Bash падает на llama-cpp; рабочий путь — Developer Shell, Ninja, `CARGO_TARGET_DIR=C:/bars-build`, `VULKAN_SDK` ([build.md](../../knowledge/build.md)). `cargo test` с примерами раз упал на занятом файле (link 1104), помог запуск `--lib --tests`.

## Дальше

Шаг 15 плана (остаток): сборка, установка, приёмка и замер на живом приложении, решение по порогу вместе с шагом 16 ([duplicates.md](../../knowledge/duplicates.md)), статистика MCP.
