---
title: Внутренняя база знаний bars
---

# Внутренняя база знаний bars


- [graph-corpus-2026-10-18/](graph-corpus-2026-10-18/Index.md) — development snapshot готового индекса, bounded graph-ресурсы и конкретные недостатки явного разбора; без инференса/holdout-приёмки.

- [graph-renderer-2026-10-18/](graph-renderer-2026-10-18/Index.md) — сравнение Cytoscape/Sigma, выбор renderer и headless-проверка макета P1; native-приёмка отдельно.

- [agent-tool-usage-2026-10-18.md](agent-tool-usage-2026-10-18.md) — качество вызовов: 14 цепочек задач, осмысленность modules/files и три сравнения областей поиска.
- [agent-tool-usage-2026-10-18/](agent-tool-usage-2026-10-18/Index.md) — исходные частоты, ручные оценки с источниками и сравнительные пробы фильтров.

Знания разработки: исследования, сравнения библиотек, замеры спайков, решения с обоснованием. Как bars устроен — в `docs/features/` и `docs/architecture/`.

- [build.md](build.md) — как собрать workspace и какие проверки гонять перед коммитом; что не проверено (macOS, CI нет).
- [stack-research.md](stack-research.md) — исследование стека: сравнения библиотек, отклонённые варианты, риски и запасные пути.
- [spike-search.md](spike-search.md) — спайк поиска на Windows: сборка, скорость (НФТ-2), качество стемминга и реранка, GPU и NPU, пороги.
- [inference-speed.md](inference-speed.md) — результаты этапа 11: FP16 rerank, полный поиск/IPC, память, отклонённый INT8, границы переноса и продуктовое внедрение.
- [inference-2026-10-18/](inference-2026-10-18/Index.md) — сырые строки, качество, мониторы и хеши: GPU/CPU-сетки, полный retrieval, worker, индексация и приёмка FP16 loader; загрязнённые timing отделены.
- [memory-usage.md](memory-usage.md) — аудит памяти, пассивный Windows-замер, различие commit/Working Set и протокол причинных опытов.
- [memory-results.md](memory-results.md) — изолированные CPU/DirectML-опыты: источник гигабайтов Private Bytes, освобождение rerank и цена повторной загрузки.
- [memory-2026-10-18/](memory-2026-10-18/Index.md) — сырые снимки памяти установленного bars и системы с ограничениями измерения.
- [memory-fp16-2026-10-18/](memory-fp16-2026-10-18/Index.md) — приёмка памяти продуктового FP16 rerank, повторная загрузка и CPU fallback.
- [inference-devices.md](inference-devices.md) — CPU, GPU и NPU: различия нагрузки, ограничения Intel/Qualcomm/Apple и поставка EP.
- [directml-performance.md](directml-performance.md) — ограничения DirectML, диагностика размещения и неверных оценок, кандидаты ускорения.
- [inference-libraries.md](inference-libraries.md) — библиотеки и runtime: OpenVINO, Windows ML, llama.cpp, Candle, Burn, tract, TEI; цена интеграции в bars.
- [search-quality-benchmark.md](search-quality-benchmark.md) — согласованная методика и запуск бенча IDEA-06 на большом проекте.
- [search-quality-results.md](search-quality-results.md) — результат IDEA-06 на i7-11700KF/RTX 3090 24 ГБ: качество, глубина, пороги, отложенная проверка и обоснование умолчаний.
- [quality-2026-10-18/](quality-2026-10-18/Index.md) — доказательства IDEA-06: выбор параметров до проверки на отложенных вопросах.
- [search-during-indexing.md](search-during-indexing.md) — замер задержки поиска во время индексации (ISS-09): очередь к модели и предел пачки.
- [spike-detach.md](spike-detach.md) — спайк запуска воркера из MCP-прокси: переживает ли он конец сессии Claude Code.
- [spike-ipc.md](spike-ipc.md) — спайк локального канала: DACL на Windows, занятое имя, тесты `bars-app`; что не проверено.
- [installer.md](installer.md) — установщик Windows: NSIS или MSI, устройство шаблона Tauri, что проверено при обновлении на месте.
- [claude-code-serena.md](claude-code-serena.md) — Serena и bars в Claude Code (ISS-08): какие инструменты Serena включены, хуки, почему агент их обходит, output style для десктопа.
- [traceability.md](traceability.md) — какой документ описывает каждое требование Т-N и НФТ.
- [local-documentation-tools.md](local-documentation-tools.md) — исследование 14 быстрых локальных инструментов: связи, дубли, целостность, контекст, история и доказательства; предложения первых спайков.
- [local-documentation-tools-audit.md](local-documentation-tools-audit.md) — критическая перепроверка исследования: источники и конкретные утверждения, готовые аналоги, исправления алгоритмов и пересмотр приоритетов.
- [graph-knowledge-retrieval.md](graph-knowledge-retrieval.md) — граф знаний, отдельный поиск отношений и путей, GraphRAG/Fast/Lazy/Light/Hippo/Path/Mini, локальные extractors и варианты Rust-интеграции.
- [graph-integration-2026-10-18.md](graph-integration-2026-10-18.md) — зачем bars GraphRAG: восемь сценариев, проект шести инструментов, изменения поиска, графический просмотр; затем архитектура и проверки.
- [graph-search-probes-2026-10-18.json](graph-search-probes-2026-10-18.json) — три качественные пробы текущего поиска для S1/S3/S6: область, параметры и выдача; не сравнительный бенч GraphRAG.
- [graph-embedding-2026-10-18.md](graph-embedding-2026-10-18.md) — первый опыт P0 без новой модели: Rust kNN на готовых векторах, восемь development-запросов, ограничения наивного expansion.
- [graph-embedding-2026-10-18/](graph-embedding-2026-10-18/Index.md) — замороженные тексты/векторы, ответы MCP, девять графов, анализ и provenance опыта P0.
- [development-ideas.md](development-ideas.md) — исследование новых направлений: польза, ограничения, отсев дублей и критерии проверки предложений.
- [development-ideas-verification.md](development-ideas-verification.md) — перепроверка DEV-01…05 по коду, поисковым продуктам и статье: подтверждения, контраргументы и исправленные выводы.
- [graph-and-relations.md](graph-and-relations.md) — направления развития: визуальный граф, отдельный поиск отношений и путей, графовый retrieval; Microsoft GraphRAG, LazyGraphRAG, HippoRAG и LightRAG.
- [memory-fp16-results.md](memory-fp16-results.md) — снижение удержания FP16, настоящий worker timer, CPU fallback и цена reload.
- [gguf-models.md](gguf-models.md) — этап 22: закреплённый llama.cpp, Qwen Q4_K_M, сборка Windows/Vulkan и методика сравнения.
- [memory-embed-2026-10-18/](memory-embed-2026-10-18/Index.md) — FP16 embed: прототип и ядровой query-путь на 100 вопросах, отказ INT8, кэш без ONNX, 219/177 тестов и незакрытые замеры индексации/парного пика; синтетика ограниченного top-K.
- [gguf-2026-10-18.json](gguf-2026-10-18.json) и [строки измерений](gguf-2026-10-18.rows.json.gz) — качество, задержки, sampled memory и CPU/Vulkan/ORT-пробы этапа 22.
