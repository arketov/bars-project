---
title: Модели bars — оглавление
---

# Модели

Откуда bars берёт векторы и оценки реранка.

- [local-models.md](local-models.md) — встроенные модели: ONNX, GGUF Q4_K_M и candle/Nomic, rerank, префиксы, кэш и отпечаток модели.
- [external-models.md](external-models.md) — удалённый провайдер и локальный OpenAI-совместимый адрес: адреса и форматы запросов, размерность, ключи, ошибки.
- [memory-plan.md](memory-plan.md) — этап 13: фазовые замеры памяти, спайк выгрузки rerank, буферы и критерии принятия оптимизаций.
- [memory-reduction.md](memory-reduction.md) — снижение памяти: реализованные FP16 rerank/lifecycle/embed, отказ INT8 embed, контракты кэша и оставшаяся приёмка.
- [inference-plan.md](inference-plan.md) — этап 11: методика измерений, результаты отбора, контракт внедрения FP16 rerank и границы приёмки.
- [device-plan.md](device-plan.md) — этап 17: отдельные устройства embed/rerank, сведения об исполнении, повторы документов, защита полной переиндексации и план приёмки.
- [device-verification.md](device-verification.md) — результаты этапа 17: регрессии, реальные CPU/DirectML-сессии, Save/Get и границы UI-проверки.
- [device-evidence.json](device-evidence.json) — устройства, выдача до и после Save, хеши исходников и бинарника этапа 17.
- [gguf-plan.md](gguf-plan.md) — этап 22: контракт встроенного llama.cpp, спайк и критерии приёмки.
