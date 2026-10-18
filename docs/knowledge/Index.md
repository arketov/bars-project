---
title: Внутренняя база знаний bars
---

# Внутренняя база знаний bars

Знания разработки: исследования, сравнения библиотек, замеры спайков, решения с обоснованием. Как bars устроен — в `docs/features/` и `docs/architecture/`.

- [build.md](build.md) — как собрать workspace и какие проверки гонять перед коммитом; что не проверено (macOS, CI нет).
- [stack-research.md](stack-research.md) — исследование стека: сравнения библиотек, отклонённые варианты, риски и запасные пути.
- [spike-search.md](spike-search.md) — спайк поиска на Windows: сборка, скорость (НФТ-2), качество стемминга и реранка, GPU и NPU, пороги.
- [search-during-indexing.md](search-during-indexing.md) — замер задержки поиска во время индексации (ISS-09): очередь к модели и предел пачки.
- [spike-detach.md](spike-detach.md) — спайк запуска воркера из MCP-прокси: переживает ли он конец сессии Claude Code.
- [spike-ipc.md](spike-ipc.md) — спайк локального канала: DACL на Windows, занятое имя, тесты `bars-app`; что не проверено.
- [installer.md](installer.md) — установщик Windows: NSIS или MSI, устройство шаблона Tauri, что проверено при обновлении на месте.
- [claude-code-serena.md](claude-code-serena.md) — Serena и bars в Claude Code (ISS-08): какие инструменты Serena включены, хуки, почему агент их обходит, output style для десктопа.
- [traceability.md](traceability.md) — какой документ описывает каждое требование Т-N и НФТ.
- [search-quality-benchmark.md](search-quality-benchmark.md) — согласованная методика и запуск бенча IDEA-06 на большом проекте.
- [search-quality-results.md](search-quality-results.md) — результат IDEA-06 на i7-11700KF/RTX 3090 24 ГБ: качество, глубина, пороги, отложенная проверка и обоснование умолчаний.
- [quality-2026-10-18/](quality-2026-10-18/Index.md) — доказательства IDEA-06: выбор параметров до проверки на отложенных вопросах.
