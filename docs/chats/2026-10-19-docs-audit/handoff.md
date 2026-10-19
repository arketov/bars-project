---
title: Аудит документации — передача дел 19-10-2026
---

# Аудит документации — передача дел

## Сделано

- Отчёты завершённых этапов перенесены из `features/` в папки сессий: этапы 11, 17, 22 (models), ISS-02 (search), IDEA-10 (clients), IDEA-08 (docs-convention). Действующие контракты влиты в документы фич.
- Слиты дубли в `knowledge/`: память → `memory.md`; DirectML и библиотеки → `inference-devices.md`; бенч IDEA-06 → `search-quality.md`; три обзора графа и проект GraphRAG → `graph-research.md` (продуктовое — в `features/graphrag/`); аудиты и перепроверки — в исходные `local-documentation-tools.md`, `development-ideas.md`.
- `architecture/`: новый `graph.md` (крейт `bars-graph`, таблицы графа), обновлены `components.md`, `schema.md`, `protocol.md` (методы и коды GraphRAG), `stack.md`, `worker.md` (очередь к модели).
- Пересобран `knowledge/Index.md` по темам, оглавления фич — таблицы со ссылкой на родителя; актуализирован `traceability.md`.
- В `AGENTS.md` добавлены правила: править документ темы вместо `*-audit.md`, отчёты этапов — в `chats/`.

## Не сделано

- Замороженные JSON-доказательства в папках `knowledge/*-2026-10-18/` упоминают старые имена документов — это данные, не правились.
- Правило про аудит-файлы не внесено в продуктовую `crates/bars-core/assets/BARS_INSTRUCTION.md`.

## Дальше

- Через несколько сессий проверить, не появляются ли снова отчёты в `features/` и парные исследования в `knowledge/`.
