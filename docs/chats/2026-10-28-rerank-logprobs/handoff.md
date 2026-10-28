---
title: Rerank через Ollama и LM Studio — итог исследования 7.1.1
---

# Rerank через Ollama и LM Studio — итог исследования 7.1.1

## Сделано

Исследованы способы rerank без штатного API. Способ согласован с автором — [ADR-0006](../../adr/0006-rerank-yes-no-logprobs.md): оценка yes/no по logprobs первого токена с промптом Qwen3-Reranker. Ollama — через `/api/generate` с `raw:true`, LM Studio — через `/v1/responses`. Допускается любая модель, Qwen3-Reranker основная. При сбое — выдача без rerank целиком. По умолчанию 4 параллельных запроса.

Запрос, оценка, отсутствующий токен, риски и отвергнутые варианты — в [исследовании Ollama](../../knowledge/ollama.md). Проба транспорта LM Studio 0.4.26 (gemma-4-e4b, RTX 3090) — в [исследовании LM Studio](../../knowledge/lmstudio.md). Изменены также external-models.md (согласованный, ещё не реализованный способ), QUESTIONS.md (OQ-227…OQ-231), Index.md папки adr.

## Не сделано

Нет реализации и аппаратной проверки на Qwen3-Reranker: весов на машине нет, D: не виден (OQ-231 открыт). Logprobs Ollama проверены только по документации, на сервере — нет.

## Сложности

- Версия Ollama с logprobs (0.12.11) взята из сторонних changelog. Проверенная ранее 0.40.9 новее, но живой проверки logprobs нет.
- Эхо `frequency_penalty: 1.1` в LM Studio при запрошенном 0 не объяснено. Это вынесено в риски [исследования LM Studio](../../knowledge/lmstudio.md).
- У gemma-4 первый токен ушёл в reasoning и `output_text` был пуст. Решено через `reasoning.effort: none`, записано там же.
- В рабочем дереве лежали незакоммиченные правки параллельных сессий в тех же PLAN.md и QUESTIONS.md. В коммит индексировались только свои куски (`git apply --cached`), rebase шёл с autostash.

## Дальше

Реализация по ADR-0006. Аналитик назвал места в коде:
- `crates/bars-core/src/models/` — новый reranker с двумя транспортами;
- `settings/raw.rs` — снять отказ для `ollama`/`lmstudio`;
- `bars-worker/src/worker/models.rs` — `load_reranker`;
- переключатель rerank в окне.

Проверки на RTX 3090 перечислены в последствиях ADR. Сначала автор отвечает на OQ-231 о весах.
