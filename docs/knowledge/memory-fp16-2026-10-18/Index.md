---
title: Приёмка памяти продуктового FP16 rerank 18-10-2026
---

# Приёмка памяти продуктового FP16 rerank

[Результаты](../memory-fp16-results.md). Четыре полных процесса: resumed download, CPU fallback, два парных worker TTL. Отклонённые/не стартовавшие процессы сохранены отдельно; третьего полного парного повтора нет.

## Файлы

- [20261018T121222122Z-cold-download-auto-rerank-1.gpu.jsonl](20261018T121222122Z-cold-download-auto-rerank-1.gpu.jsonl) — внешний GPU-сэмплер и пересечения с компиляцией.
- [20261018T121222122Z-cold-download-auto-rerank-1.jsonl](20261018T121222122Z-cold-download-auto-rerank-1.jsonl) — нативный фазовый ряд памяти и оценок.
- [20261018T121222122Z-cold-download-auto-rerank-1.process.json](20261018T121222122Z-cold-download-auto-rerank-1.process.json) — код завершения owned-процесса.
- [20261018T121222122Z-cold-download-auto-rerank-1.stderr.txt](20261018T121222122Z-cold-download-auto-rerank-1.stderr.txt) — диагностика процесса.
- [20261018T123317287Z-resumed-download-auto-rerank-1.gpu.jsonl](20261018T123317287Z-resumed-download-auto-rerank-1.gpu.jsonl) — внешний GPU-сэмплер и пересечения с компиляцией.
- [20261018T123317287Z-resumed-download-auto-rerank-1.jsonl](20261018T123317287Z-resumed-download-auto-rerank-1.jsonl) — нативный фазовый ряд памяти и оценок.
- [20261018T123317287Z-resumed-download-auto-rerank-1.process.json](20261018T123317287Z-resumed-download-auto-rerank-1.process.json) — код завершения owned-процесса.
- [20261018T123317287Z-resumed-download-auto-rerank-1.stderr.txt](20261018T123317287Z-resumed-download-auto-rerank-1.stderr.txt) — диагностика процесса.
- [20261018T125547398Z-resumed-download-final-auto-rerank-1.gpu.jsonl](20261018T125547398Z-resumed-download-final-auto-rerank-1.gpu.jsonl) — внешний GPU-сэмплер и пересечения с компиляцией.
- [20261018T125547398Z-resumed-download-final-auto-rerank-1.jsonl](20261018T125547398Z-resumed-download-final-auto-rerank-1.jsonl) — нативный фазовый ряд памяти и оценок.
- [20261018T125547398Z-resumed-download-final-auto-rerank-1.process.json](20261018T125547398Z-resumed-download-final-auto-rerank-1.process.json) — код завершения owned-процесса.
- [20261018T125547398Z-resumed-download-final-auto-rerank-1.stderr.txt](20261018T125547398Z-resumed-download-final-auto-rerank-1.stderr.txt) — диагностика процесса.
- [20261018T130140009Z-worker-ttl-auto-bounded-1.gpu.jsonl](20261018T130140009Z-worker-ttl-auto-bounded-1.gpu.jsonl) — внешний GPU-сэмплер и пересечения с компиляцией.
- [20261018T130140009Z-worker-ttl-auto-bounded-1.jsonl](20261018T130140009Z-worker-ttl-auto-bounded-1.jsonl) — нативный фазовый ряд памяти и оценок.
- [20261018T130140009Z-worker-ttl-auto-bounded-1.process.json](20261018T130140009Z-worker-ttl-auto-bounded-1.process.json) — код завершения owned-процесса.
- [20261018T130140009Z-worker-ttl-auto-bounded-1.stderr.txt](20261018T130140009Z-worker-ttl-auto-bounded-1.stderr.txt) — диагностика процесса.
- [20261018T130225203Z-corrupt-config-auto-rerank-1.gpu.jsonl](20261018T130225203Z-corrupt-config-auto-rerank-1.gpu.jsonl) — внешний GPU-сэмплер и пересечения с компиляцией.
- [20261018T130225203Z-corrupt-config-auto-rerank-1.jsonl](20261018T130225203Z-corrupt-config-auto-rerank-1.jsonl) — нативный фазовый ряд памяти и оценок.
- [20261018T130225203Z-corrupt-config-auto-rerank-1.process.json](20261018T130225203Z-corrupt-config-auto-rerank-1.process.json) — код завершения owned-процесса.
- [20261018T130225203Z-corrupt-config-auto-rerank-1.stderr.txt](20261018T130225203Z-corrupt-config-auto-rerank-1.stderr.txt) — диагностика процесса.
- [20261018T130604982Z-worker-ttl-v2-auto-bounded-1.gpu.jsonl](20261018T130604982Z-worker-ttl-v2-auto-bounded-1.gpu.jsonl) — внешний GPU-сэмплер и пересечения с компиляцией.
- [20261018T130604982Z-worker-ttl-v2-auto-bounded-1.jsonl](20261018T130604982Z-worker-ttl-v2-auto-bounded-1.jsonl) — нативный фазовый ряд памяти и оценок.
- [20261018T130604982Z-worker-ttl-v2-auto-bounded-1.process.json](20261018T130604982Z-worker-ttl-v2-auto-bounded-1.process.json) — код завершения owned-процесса.
- [20261018T130604982Z-worker-ttl-v2-auto-bounded-1.stderr.txt](20261018T130604982Z-worker-ttl-v2-auto-bounded-1.stderr.txt) — диагностика процесса.
- [20261018T130604982Z-worker-ttl-v2-auto-bounded-2.gpu.jsonl](20261018T130604982Z-worker-ttl-v2-auto-bounded-2.gpu.jsonl) — внешний GPU-сэмплер и пересечения с компиляцией.
- [20261018T130604982Z-worker-ttl-v2-auto-bounded-2.jsonl](20261018T130604982Z-worker-ttl-v2-auto-bounded-2.jsonl) — нативный фазовый ряд памяти и оценок.
- [20261018T130604982Z-worker-ttl-v2-auto-bounded-2.process.json](20261018T130604982Z-worker-ttl-v2-auto-bounded-2.process.json) — код завершения owned-процесса.
- [20261018T130604982Z-worker-ttl-v2-auto-bounded-2.stderr.txt](20261018T130604982Z-worker-ttl-v2-auto-bounded-2.stderr.txt) — диагностика процесса.
- [20261018T130716793Z-worker-ttl-repeat-auto-bounded-1.gpu.jsonl](20261018T130716793Z-worker-ttl-repeat-auto-bounded-1.gpu.jsonl) — внешний GPU-сэмплер и пересечения с компиляцией.
- [20261018T130716793Z-worker-ttl-repeat-auto-bounded-1.jsonl](20261018T130716793Z-worker-ttl-repeat-auto-bounded-1.jsonl) — нативный фазовый ряд памяти и оценок.
- [20261018T130716793Z-worker-ttl-repeat-auto-bounded-1.process.json](20261018T130716793Z-worker-ttl-repeat-auto-bounded-1.process.json) — код завершения owned-процесса.
- [20261018T130716793Z-worker-ttl-repeat-auto-bounded-1.stderr.txt](20261018T130716793Z-worker-ttl-repeat-auto-bounded-1.stderr.txt) — диагностика процесса.
- [fallback-fixture.json](fallback-fixture.json) — замороженный вход или описание отказа.
- [fixture.json](fixture.json) — замороженный вход или описание отказа.
- [preflight-20261018T121222122Z.jsonl](preflight-20261018T121222122Z.jsonl) — проверки запаса и чужой нагрузки.
- [preflight-20261018T123317287Z.jsonl](preflight-20261018T123317287Z.jsonl) — проверки запаса и чужой нагрузки.
- [preflight-20261018T124313165Z.jsonl](preflight-20261018T124313165Z.jsonl) — проверки запаса и чужой нагрузки.
- [preflight-20261018T125547398Z.jsonl](preflight-20261018T125547398Z.jsonl) — проверки запаса и чужой нагрузки.
- [preflight-20261018T130140009Z.jsonl](preflight-20261018T130140009Z.jsonl) — проверки запаса и чужой нагрузки.
- [preflight-20261018T130225203Z.jsonl](preflight-20261018T130225203Z.jsonl) — проверки запаса и чужой нагрузки.
- [preflight-20261018T130604982Z.jsonl](preflight-20261018T130604982Z.jsonl) — проверки запаса и чужой нагрузки.
- [preflight-20261018T130716793Z.jsonl](preflight-20261018T130716793Z.jsonl) — проверки запаса и чужой нагрузки.
- [run-20261018T121222122Z.json](run-20261018T121222122Z.json) — хеши бинарника, источников и входа на момент запуска.
- [run-20261018T123317287Z.json](run-20261018T123317287Z.json) — хеши бинарника, источников и входа на момент запуска.
- [run-20261018T124313165Z.json](run-20261018T124313165Z.json) — хеши бинарника, источников и входа на момент запуска.
- [run-20261018T125547398Z.json](run-20261018T125547398Z.json) — хеши бинарника, источников и входа на момент запуска.
- [run-20261018T130140009Z.json](run-20261018T130140009Z.json) — хеши бинарника, источников и входа на момент запуска.
- [run-20261018T130225203Z.json](run-20261018T130225203Z.json) — хеши бинарника, источников и входа на момент запуска.
- [run-20261018T130604982Z.json](run-20261018T130604982Z.json) — хеши бинарника, источников и входа на момент запуска.
- [run-20261018T130716793Z.json](run-20261018T130716793Z.json) — хеши бинарника, источников и входа на момент запуска.
- [summary.json](summary.json) — пересчитанные метрики и отклонённые прогоны.
