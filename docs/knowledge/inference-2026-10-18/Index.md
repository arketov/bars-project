---
title: Доказательства этапа 11
---

# Доказательства этапа 11

Статус: измерительная часть и продуктовое внедрение FP16 rerank завершены. Исследовательский финал — 138 вопросов и 44 IPC-ответа; приёмка продуктового loader — отдельно 138 core и 22 IPC-ответа без новых потерь/fallback. Испытанная сборка имеет свои хеши; последующие параллельные изменения общих исходников отражены в `sourceDrift`. Embed-screen и четыре индексации завершены. Загрязнённые timing отделены; естественный корпус5000 и другие платформы не проверены.

[← База знаний](../Index.md)

- [baseline.json](baseline.json) — сводка начальных FP32 CPU16 и DirectML; качество только на screen-выборке. Поле timingValid старого монитора означает отсутствие обнаруженных других бенчей/сборок, а не доказанную свободу RAM/commit.
- [cpu16-rows.jsonl.gz](cpu16-rows.jsonl.gz) — сырые строки CPU16.
- [dml-rows.jsonl.gz](dml-rows.jsonl.gz) — сырые строки DirectML.
- [cpu16-run.json](cpu16-run.json) — конфигурация, сессия, хеши и отметка завершения CPU16.
- [dml-run.json](dml-run.json) — такие же метаданные DirectML.
- [recovery-check.json](recovery-check.json) — повтор DirectML после освобождения памяти: 30 наблюдений, логиты совпали с первым запуском; время загрязнено фоновыми rustc.
- [recovery-dml-rows.jsonl.gz](recovery-dml-rows.jsonl.gz) — сырые строки повтора.
- [recovery-dml-run.json](recovery-dml-run.json) — параметры и хеши повтора.
- [recovery-dml-monitor.json](recovery-dml-monitor.json) — системная RAM/commit и зафиксированные фоновые rustc.
- [model-fp16.json](model-fp16.json) — pinned revision, размер и проверенный SHA256 FP16; инференс этого варианта ещё не выполнен.
- [model-quantized.json](model-quantized.json) — такие же метаданные INT8; наличие весов не доказывает совместимость или качество.
- [cpu16-monitor.json](cpu16-monitor.json) — монитор CPU16 без системных счётчиков памяти.
- [dml-monitor.json](dml-monitor.json) — такой же монитор DirectML.
- [profile-cpu.json](profile-cpu.json) — операторы и провайдеры CPU-профиля.
- [profile-dml.json](profile-dml.json) — операторы и провайдеры DirectML-профиля. Профили выполнялись при фоновых сборках; длительности trace не являются чистым сравнением скорости.
- [hardware.json](hardware.json) — окружение и хеш архива fastembed, исходники которого совпали с локальной копией.
- [holdout-frozen.json](holdout-frozen.json) — фиксация дополнительного малого набора до просмотра precision/length-вариантов.
- [memory-blocker.json](memory-blocker.json) — системная память в момент блокировки.
- [cpu1-invalidated.json](cpu1-invalidated.json) — прерванный пилот CPU1 исключён из чистого сравнения; завершённым прогоном не считается.
- [checksums.json](checksums.json) — SHA256 файлов доказательств, кроме самого списка.
- [gpu-screen.json](gpu-screen.json) — чистая сетка 11 вариантов, по 30 наблюдений на тех же 10 train-вопросах; скорость, качество, потери и повтор baseline. Это отбор, не финальная проверка качества.
- [gpu-screen.tar.gz](gpu-screen.tar.gz) — исходные строки, конфигурации, сессии, завершения и внешние мониторы 11 вариантов; папки внутри архива сохраняют структуру стенда.
- [gpu-combos.json](gpu-combos.json) — screen-комбинации FP16/batch16/фиксированной формы на новом бинарнике; 5 × 30 наблюдений.
- [gpu-combos.tar.gz](gpu-combos.tar.gz) — исходные данные этих комбинаций, включая dedicated/shared GPU memory и редкие снимки температуры/частот NVIDIA.
- [cpu-pilot.json](cpu-pilot.json) — CPU1/2/4/8/16 и INT8 CPU4: только три фиксированных train-вопроса на вариант; это пилот, не финал.
- [cpu-pilot.tar.gz](cpu-pilot.tar.gz) — исходные строки, конфигурации и мониторы CPU-пилота.
- [work-reduction.json](work-reduction.json) — офлайн-симуляция K и предварительного порога по замороженным C30/K15 оценкам этапа 9; нового времени инференса не содержит.
- [full-retrieval.json](full-retrieval.json) — штатный FP32 против FP16/batch16: полный поиск, качество и время на трёх наборах, всего 276 ответов.
- [full-retrieval.tar.gz](full-retrieval.tar.gz) — исходные строки полного поиска, конфигурации, сессии, завершения и мониторы; тексты исходных документов в строки не входят.
- [worker.json](worker.json) — настоящий IPC: по 22 ответа, n каждого режима, медиана/максимум и проверка качества.
- [worker-metadata.json](worker-metadata.json) — хеш бинарника worker, хеши оригинальных строк, завершения и мониторы.
- [worker-rows.jsonl.gz](worker-rows.jsonl.gz) — структурные ответы IPC и вычисленные метрики; тела документов заменены SHA256. Полные оригиналы остаются локально в ignored runs.
- [embedding.json](embedding.json) — screen коротких запросов и 64 фиксированных текстов, cosine/throughput/память; не доказательство retrieval precision-экспорта.
- [embedding-artifacts.json](embedding-artifacts.json) — закреплённые FP16/INT8 embed-экспорты, SHA256, tokenizer, CLS/нормализация и размерность.
- [indexing.json.gz](indexing.json.gz) — четыре настоящих полных/неизменённых/инкрементальных прохода, конфигурации и мониторы. Чистые timing только двух GPU-вариантов; оба CPU-варианта функционально успешны, но с фоновой нагрузкой.
- [cpu-int8-proof.json.gz](cpu-int8-proof.json.gz) — первичный counterexample INT8 `od-010`, ранняя вспомогательная остановка `od-001`, CPU FP32 reference и дополнительные пилоты; качество-only timing не считается чистым.
- [base-models.json](base-models.json) — SHA256 исходных FP32 весов и tokenizer/config, закреплённые baseline snapshots.
- [verification.json](verification.json) — финальные 197 core / 142 worker / реальный CPU FP32, SHA256 журналов тестов, ручной просмотр и границы непроведённой приёмки.
- [product-acceptance.json.gz](product-acceptance.json.gz) — приёмка продуктового FP16 loader: 138 core и 22 IPC-ответа, метрики/fallback, хеши исходников/бинарников и мониторы; timing quality-only исключён из чистой оценки скорости.
- [product-rows.jsonl.gz](product-rows.jsonl.gz) — 160 продуктовых ответов и метрики; тела документов IPC заменены SHA256.

Полные входы, тяжёлые профили и скачанные модели остаются в исключённых из Git каталогах `spikes/inference/`. Методика — [проект измерений](../../chats/2026-10-18-inference-design/stage11-plan.md); трактовка — [исследование скорости](../inference-speed.md).
