---
title: Передача дел 16-10-2026 — ISS-01, загрузка модели
---

# Передача дел 16-10-2026 — ISS-01

## Сделано

- Каталог знает полный список файлов каждой встроенной модели и её размер: `is_downloaded` проверяет все файлы, `download_percent` считает ход по `blobs/` (`bars-core/src/models/catalog.rs`).
- Скачивание и загрузка идут под одним замком (`Loads` в `bars-worker/src/worker/models.rs`); поиск замок не ждёт и отвечает `model_loading` с `data.percent`, rerank в это время пропускается с причиной в `rerank_error`.
- Окно показывает «Модель грузится, N %» и повторяет поиск раз в две секунды.
- Документы: `local-models.md`, `protocol.md`, `worker.md`, `error-states.md`, `implementation.md`. Тесты: каталог, замок и фоновая загрузка в `models.rs`, `model_loading` в обработчике, `errors.test.ts`.

## Не сделано

- Проверка на чистом `~/.bars/models` через `BARS_USER_HOME` с настоящим скачиванием (около 2 ГБ): не запускалась.
- Размеры моделей в каталоге приблизительные для Qwen3 и Nomic (проверены только bge-m3 и rerank).
- Не коммитились чужие правки рабочей копии: `QUESTIONS.md`, `docs/BARS_INSTRUCTION.md`, `docs/user_ideas.md`, handoff сессии plan-review, `bench_quality.rs`.

## Сложности

- Первая версия теста не ловила блокировку: `is_loaded` ждал мьютекс загрузчика (см. log.md).

## Дальше

- Пункт 2 плана (ISS-06 и IDEA-07): тест на сотни файлов разом, потом исправление слежения.
