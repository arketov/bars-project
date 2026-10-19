---
title: Внутренняя база знаний bars
---

# Внутренняя база знаний bars

[← Документация](../Index.md)

Исследования, сравнения, замеры и соглашения разработки. Как bars устроен сейчас — в [features/](../features/Index.md) и [architecture/](../architecture/Index.md); исследование не подтверждает внедрение предложенного. Сырые данные замеров лежат в папках с датой в имени рядом с документом, который их объясняет.

## Разработка и стек bars

| Файл | Когда читать |
|---|---|
| [build.md](build.md) | Собрать проект, прогнать проверки перед коммитом и тесты со встроенной моделью |
| [stack-research.md](stack-research.md) | Почему выбраны крейты стека, какие варианты отклонены, риски и их исход |
| [spike-detach.md](spike-detach.md) | Переживает ли воркер, запущенный из `bars-mcp`, конец сессии агента |
| [spike-ipc.md](spike-ipc.md) | Как проверены DACL и занятое имя локального канала на Windows |
| [installer.md](installer.md) | Почему NSIS, что пришлось обойти в шаблоне Tauri и что проверено руками |
| [traceability.md](traceability.md) | Какой документ описывает конкретное Т-N или НФТ |

## Качество и скорость поиска

| Файл | Когда читать |
|---|---|
| [spike-search.md](spike-search.md) | Первый спайк поиска на ноутбуке: сборка, скорость поиска и embed, rerank, качество на корпусе bars |
| [search-quality.md](search-quality.md) | Методика и результаты бенча IDEA-06: качество, глубина, пороги, обоснование умолчаний |
| [quality-2026-10-18/](quality-2026-10-18/Index.md) | Сырые данные бенча IDEA-06 |
| [search-during-indexing.md](search-during-indexing.md) | Задержка запроса во время индексации и эффект очереди к модели (ISS-09) |

## Инференс, модели и память

| Файл | Когда читать |
|---|---|
| [inference-speed.md](inference-speed.md) | Скорость инференса: FP16 rerank против FP32, worker и конкуренция, отклонённые методы, протокол замеров |
| [inference-2026-10-18/](inference-2026-10-18/Index.md) | Сырые данные этапа 11: GPU/CPU-сетки, полный retrieval, worker, индексация |
| [inference-devices.md](inference-devices.md) | CPU, GPU и NPU, ограничения DirectML, диагностика ISS-03, кандидаты EP и библиотек |
| [gguf-models.md](gguf-models.md) | Qwen3 GGUF и llama.cpp: качество против bge-m3, память, сборка Windows/Vulkan |
| [gguf-2026-10-18/](gguf-2026-10-18/Index.md) | Сырые данные этапа 22: сводка и строки измерений |
| [memory.md](memory.md) | Память bars: метрики, причины пиков, FP16 rerank и выгрузка, FP16/INT8 embed, подкачка |
| [memory-2026-10-18/](memory-2026-10-18/Index.md) | Сырые снимки памяти FP32-опытов |
| [memory-fp16-2026-10-18/](memory-fp16-2026-10-18/Index.md) | Сырые данные приёмки памяти FP16 rerank, повторной загрузки и CPU fallback |
| [memory-embed-2026-10-18/](memory-embed-2026-10-18/Index.md) | Сырые данные FP16/INT8 embed и ограниченного хранения оценок |

## Граф знаний и GraphRAG

| Файл | Когда читать |
|---|---|
| [graph-research.md](graph-research.md) | Зачем bars граф и поиск связей, методы GraphRAG, локальный смысловой слой, хранение, проверка пользы |
| [graph-embedding-2026-10-18.md](graph-embedding-2026-10-18.md) | Опыт графа близости на готовых векторах: метод, результат, почему наивное расширение не принято |
| [graph-embedding-2026-10-18/](graph-embedding-2026-10-18/Index.md) | Сырые данные этого опыта: тексты, векторы, графы, анализ |
| [graph-search-probes-2026-10-18/](graph-search-probes-2026-10-18/Index.md) | Три пробы обычного поиска для S1/S3/S6 до GraphRAG |
| [graph-corpus-2026-10-18/](graph-corpus-2026-10-18/Index.md) | Development-снимок индекса, ресурсы графа и недостатки явного разбора |
| [graph-renderer-2026-10-18/](graph-renderer-2026-10-18/Index.md) | Сравнение Cytoscape и Sigma, headless-проверка макета графа |

## Идеи развития и агенты

| Файл | Когда читать |
|---|---|
| [development-ideas.md](development-ideas.md) | Улучшения надёжности и диагностики поиска DEV-01…05, их основания и перепроверка |
| [duplicates.md](duplicates.md) | IDEA-13: как различать точные, близкие, частичные, смысловые и шаблонные повторы, пороги и задержка |
| [duplicates-2026-10-19/](duplicates-2026-10-19/Index.md) | Сырые данные исследования повторов: набор, разметка, метрики, пороги |
| [new-topics-threshold-2026-10-19.md](new-topics-threshold-2026-10-19.md) | IDEA-14: порог слияния вопросов для «Новых тем» на bge-m3 (0,79 с лексическими проверками), что не различает косинус |
| [new-topics-threshold-2026-10-19/](new-topics-threshold-2026-10-19/Index.md) | Сырые данные калибровки: пары, анализ, проверки, векторы |
| [local-documentation-tools.md](local-documentation-tools.md) | 14 идей быстрых локальных инструментов документации, сравнение с аналогами, источники |
| [agent-tool-usage-2026-10-18.md](agent-tool-usage-2026-10-18.md) | Как агенты вызывают bars и как выбирать область поиска |
| [agent-tool-usage-2026-10-18/](agent-tool-usage-2026-10-18/Index.md) | Сырые частоты, ручные оценки и пробы фильтров |
| [claude-code-serena.md](claude-code-serena.md) | ISS-08: Serena, хуки и output style в Claude Code |
