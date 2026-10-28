---
title: Rerank через Ollama и LM Studio — находки
---

# Rerank через Ollama и LM Studio — находки

28-10-2026: диск D: с тестовыми моделями не виден, portable Ollama удалён, в LM Studio нет reranker-моделей. Аппаратная проба reranker невозможна без скачивания; вопрос — OQ-231.
28-10-2026: LM Studio 0.4.26 отдаёт logprobs только в `/v1/responses`; `/v1/completions` и `/v1/chat/completions` возвращают `null`. Подробности — [исследование LM Studio](../../knowledge/lmstudio.md).
28-10-2026: способ выбран с независимой рекомендацией аналитика (Plan, opus) и согласован автором — [ADR-0006](../../adr/0006-rerank-yes-no-logprobs.md).
