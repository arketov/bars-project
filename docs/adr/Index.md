---
title: Решения bars (ADR) — оглавление
---

# Решения bars

[← Документация](../Index.md)

Принятые решения с обоснованием, по одному в файле `NNNN-<тема>.md`. Решения до 15-10-2026 записаны в QUESTIONS.md и в документах `docs/`.

- [0001-docs-by-features.md](0001-docs-by-features.md) — документация bars по фичам, журнал сессий, MAIN_IDEA в `docs/`.
- [0002-qwen-gguf.md](0002-qwen-gguf.md) — встроенный llama.cpp для Qwen Q4_K_M, отдельный отпечаток и сохранение Nomic на candle.
- [0003-window-docking.md](0003-window-docking.md) — accepted, 20-10-2026: Dockview с прямым адаптером Svelte применён в каркасе 1.1.4; отдельные окна Tauri поверх общего воркера остаются в проектировании редактора.
- [0004-graph-physics-renderer.md](0004-graph-physics-renderer.md) — proposed, 21-10-2026: d3-force в worker + Canvas2D для новой реализации графа; замеры обоих корпусов, условия пересмотра.
- [0005-editor-codemirror.md](0005-editor-codemirror.md) — proposed, 22-10-2026: редактор Markdown на CodeMirror 6; замер и устройство — исследование и проект редактора.
- [0006-rerank-yes-no-logprobs.md](0006-rerank-yes-no-logprobs.md) — accepted, 28-10-2026: rerank через Ollama и LM Studio по logprobs yes/no с Qwen3-Reranker; без частичного rerank, 4 параллельных запроса.
