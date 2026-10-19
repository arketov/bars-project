---
title: Передача сессии — ID чанка и get_similar
---

# Передача сессии — ID чанка и get_similar

## Сделано

- Шаг 14: формат, срок действия, ошибки и видимость ID чанка — [maintenance-tools.md](../../features/search/maintenance-tools.md#идентификатор-чанка-для-get_similar); формат принят автором.
- Шаг 15 (код): `chunk_id` в выдаче поиска; ядро `search/similar.rs`; метод `similar`, ошибка `chunk_not_found` (-32016); воркер, MCP `bars_get_similar`, CLI `bars-cli similar`; учёт вызовов в статистике MCP (allowlist, итог `fragments`, фильтр инструментов экрана, подписи).
- Тесты: ядро (6), воркер (сквозной через канал для CLI и MCP), протокол, MCP (в том числе запись в статистику и `chunk_not_found`), экран статистики; все проходят (`cargo test --release --lib --tests`, vitest).
- Документы: protocol, mcp-tools, cli, hybrid-search, maintenance-tools, mcp-statistics (матрица и охват), search/Index, manual/agents, manual/terminal, CHANGELOG, макет статистики, ideas (IDEA-12), PLAN. Skill `bars-docs` и стартовая инструкция уже описывали инструмент (правка параллельной сессии).
- Правки вошли в коммит 80b09b8 вместе с чужими; последние правки (статистика, документы) — в рабочей копии, не закоммичены.

## Не сделано

Приёмка на настоящей bge-m3 и в живом приложении, замер задержки, порог близости (по умолчанию его нет). Установленный bars старее репозитория: инструмент появится после сборки и установки. Нативная проверка экрана статистики с новым инструментом — в этапе приёмки окна (ISS-14).

## Сложности

Параллельная сессия (`get_duplicates`) правит те же файлы и тот же счётчик инструментов в `tests/mcp.rs`: общие файлы перечитывать перед правкой. Сборка из Git Bash падает на llama-cpp; рабочий путь — Developer Shell, Ninja, `CARGO_TARGET_DIR=C:/bars-build`, `VULKAN_SDK` ([build.md](../../knowledge/build.md)). `cargo test` с примерами раз упал на занятом файле (link 1104): помог запуск `--lib --tests`.

## Дальше

Шаг 15 плана (остаток): сборка, установка, приёмка и замер на живом приложении; порог близости решать вместе с `get_duplicates` ([duplicates.md](../../knowledge/duplicates.md)).
