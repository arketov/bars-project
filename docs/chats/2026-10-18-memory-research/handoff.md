---
title: Передача сессии — этап 13, снижение памяти
---

# Передача сессии — этап 13, снижение памяти

## Сделано

- Предыдущая приёмка FP16 rerank/TTL: knowledge/memory-fp16-results.md; worker-пара 6,87 → 5,54–5,56 ГиБ, после drop обеих 225–230 МиБ.
- Уменьшены менее крупные статьи: top-C/оценки текстовых кандидатов вместо O(N), BLOB без двух Vec; скачивание bge-m3 без создания сессии. Предыдущие проверки 200 core / 170 worker / clippy сохранены отдельно.
- Реализован Windows-профиль BAAI/bge-m3:fp16: pinned Xenova revision/BLAKE3, artifact-only download, атомарная отметка кэша, fingerprint precision+revision. DirectML обязателен; CPU fallback запрещён для отдельного FP16-профиля. Прежняя FP32 bge-m3/default/CPU fallback сохранены. UI выбирает DirectML и предупреждает о переиндексации Т-25.
- Прототип FP16 и новый ядровой query loader: по 100 вопросов, новых потерь всех выбранных метрик при 0/0,1 нет относительно product3-overdrive. Все 102400 query-компонентов совпали. Документные векторы пока экспериментальные; адаптация fingerprint в собственной копии НЕ считается свежей индексацией. INT8 embed отклонён по od-001: MRR@10 0,125→0, независимо от отказа INT8 rerank.
- Настоящая проверка cached FP16-артефактов без ONNX: peak 3,52 МиБ; изменённый config отклонён BLAKE3/ready=false. Это не холодное сетевое скачивание. Raw gzip, SHA256, бинарники/исходники и ограничения: knowledge/memory-embed-2026-10-18/Index.md.
- Совместная release-сборка core/worker стендов прошла; финально 219 core + 177 worker lib, 10 DevicePicker, Svelte check 0/0. Ручная проверка выполнена. Новый clippy не завершён: общий build-lock занят, свой ожидающий процесс остановлен до запуска компилятора. Нельзя выдавать прежний clippy за проверку нового FP16-кода.

## Не сделано

- Свежая полная индексация ядровым FP16-профилем; retrieval bars/holdout на новых индексах; настоящий парный FP16 embed/rerank peak/TTL, чистая скорость. Два index preflight 120/180 с и worker preflight 90 с не дали старт. Исходные core indexing процессы обходят padded-оболочку worker; их память не подменяет worker-пик.
- Отдельный baseline окна, allocator tuning, Mac/ноутбук. Этап 13 открыт. Приложение не устанавливалось/не обновлялось. Собственных commit/push нет; чужие main/dirty изменения сохранены.

## Сложности

- Общий target C:/bg22 многократно пересобирался при параллельных GGUF/graph изменениях. Vulkan SDK уже установлен в %LOCALAPPDATA%/bars-spikes/VulkanSDK/1.4.363.0; helper среды spikes/memory/runs/build-env.ps1, CMAKE_BUILD_PARALLEL_LEVEL=1. Промежуточный build попал между добавлением graph-модуля и зависимости; повтор прошёл, чужой код не откатывался.
- Свободный commit колебался примерно 3,4–8 ГиБ; модельные резервы не снижались. После жалобы автора прекращены дополнительные циклы проверок. Финальные lib-тесты прошли; зависший в очереди clippy остановлен только в собственном дереве.

## Дальше

- Последнее указание автора: работающий bars НЕ закрывать. PID 9928 не трогали; профиль/настройки/рабочие индексы не меняли. Собственных модельных/компиляционных процессов больше нет.
- Сначала запас памяти. Native startup: FP16 quality commit6/RAM4; core index runner commit9/RAM8; worker pair commit8/RAM4, дальше фазовые reserve6/5. Emergency RAM512MiB/commit1GiB. Не повторять пустые ожидания без изменения ресурса.
- Замороженные бинарники spikes/memory/bin/core-v1/{bench_inference,bench_memory,bench_worker_memory}.exe с vulkan-1.dll. Кэш spikes/memory/embed-profiles/core-v1/models, config core-fp16.json рядом. Runner нормализует ModelsPath, MemoryOnly разрешает фоновые сборки с сохранением резервов и явной непригодностью времени; его новый режим ещё не запускался.
- Новый index: spikes/inference/run-series.ps1, Mode index, Data spikes/inference/data/overdrive, config core-fp16.json, новый OutputRoot. Затем quality на full-data; reference ТОЛЬКО spikes/inference/runs/product3-overdrive/production/rows.jsonl (старый full-overdrive — прежний FP32 rerank).
- Worker-пара: scripts/run-memory-spike.ps1, bench_worker_memory.exe, Devices auto, Modes bounded, Fp16Embed; собственная папка evidence с копией knowledge/memory-2026-10-18/fixture.json. build-pair-v2.json проверяет SHA256. До измеренного результата память пары не обещать.

Документация синхронизирована после итогового запроса автора: memory-usage/results, memory-plan/reduction, local-models, settings, оглавления, OQ-79 и текущие оставшиеся шаги. Новых модельных запусков/сборок в документационной правке нет.
