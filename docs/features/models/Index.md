---
title: Модели bars — оглавление
---

# Модели

← Фичи bars (`features/Index.md`). Откуда bars берёт векторы и оценки реранка.

| Файл | Когда читать |
|---|---|
| `local-models.md` | Встроенные модели и действующая приёмка; отдельно — проект CUDA 8.1.3: зависимости, поставка, явный выбор GPU, ограниченный откат, ускорители и численная приёмка; ожидает согласования |
| `external-models.md` | HTTP-провайдеры, Ollama, LM Studio, TEI, llama.cpp и Infinity: форматы, размерность, ключи, ошибки и границы проверки |
| `memory-plan.md` | Отложенная приёмка установленной версии после закрытия реализации 4.2.2, запрет на обновление рабочего bars и границы принятой меры |

Замеры, методика и исследования по моделям лежат в базе знаний (`knowledge/Index.md`): скорость инференса (`knowledge/inference-speed.md`), память (`knowledge/memory.md`), устройства и библиотеки (`knowledge/inference-devices.md`), GGUF (`knowledge/gguf-models.md`). Отчёты завершённых этапов 11, 17 и 22 — в журналах сессий inference-design (`chats/2026-10-18-inference-design/stage11-plan.md`), device-design (`chats/2026-10-18-device-design/stage17-plan.md`), gguf-models (`chats/2026-10-18-gguf-models/stage22-plan.md`).
