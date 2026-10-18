---
title: Внутренняя база знаний bars
---

# Внутренняя база знаний bars

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
- [graph-integration-2026-10-18.md](graph-integration-2026-10-18.md) — внедрение GraphRAG: разделы и чанки, схема и обновление связей, MCP/CLI, четыре вида графа в стиле Obsidian, Cytoscape/Sigma и этапы проверки.
- [development-ideas.md](development-ideas.md) — исследование новых направлений: польза, ограничения, отсев дублей и критерии проверки предложений.
- [development-ideas-verification.md](development-ideas-verification.md) — перепроверка DEV-01…05 по коду, поисковым продуктам и статье: подтверждения, контраргументы и исправленные выводы.
- [graph-and-relations.md](graph-and-relations.md) — направления развития: визуальный граф, отдельный поиск отношений и путей, графовый retrieval; Microsoft GraphRAG, LazyGraphRAG, HippoRAG и LightRAG.
- [memory-fp16-results.md](memory-fp16-results.md) — снижение удержания FP16, настоящий worker timer, CPU fallback и цена reload.
