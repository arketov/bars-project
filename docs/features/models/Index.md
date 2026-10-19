---
title: Модели bars — оглавление
---

# Модели

[← Фичи bars](../Index.md). Откуда bars берёт векторы и оценки реранка.

| Файл | Когда читать |
|---|---|
| [local-models.md](local-models.md) | Встроенные модели: каталог (bge-m3 FP32/FP16, Qwen GGUF, Nomic), rerank и его precision, префиксы и длина входа, кэш и «скачано», выгрузка по простою, отпечаток модели, устройства, приёмка FP16 rerank |
| [external-models.md](external-models.md) | Удалённый провайдер и локальный OpenAI-совместимый адрес: адреса и форматы запросов, размерность, ключи, ошибки и повторы |
| [memory-plan.md](memory-plan.md) | Снижение памяти (этап открыт): что уже сделано, что осталось принять, критерии и ограничения стенда |

Замеры, методика и исследования по моделям лежат в [базе знаний](../../knowledge/Index.md): [скорость инференса](../../knowledge/inference-speed.md), [память](../../knowledge/memory.md), [устройства и библиотеки](../../knowledge/inference-devices.md), [GGUF](../../knowledge/gguf-models.md). Отчёты завершённых этапов 11, 17 и 22 — в журналах сессий [inference-design](../../chats/2026-10-18-inference-design/stage11-plan.md), [device-design](../../chats/2026-10-18-device-design/stage17-plan.md), [gguf-models](../../chats/2026-10-18-gguf-models/stage22-plan.md).
