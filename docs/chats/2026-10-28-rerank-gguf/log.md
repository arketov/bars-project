---
title: Журнал — rerank-модели в GGUF (8.2.1)
---

- Находка: ранние GGUF Qwen3-Reranker (mradermacher, QuantFactory, июнь 2025) без `cls.output` и шаблона `rerank` — режим rank pooling с ними не работает; годны только конверсии после поддержки в llama.cpp (ggml-org, Voodisss, giladgd, gorse-io).
- Находка: закреплённый llama.cpp не грузит голову классификатора у gemma/qwen2/new — bge-reranker-v2-gemma, mxbai-rerank-v2 и gte-multilingual-reranker через rank pooling недоступны.
