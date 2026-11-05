---
title: Handoff — wait_models и тест ISS-59 на новой карточке, шаг 2.4.3
---

# Handoff — wait_models и тест ISS-59 на новой карточке, шаг 2.4.3

Продолжает [handoff выдачи окна](../2026-11-05-search-results-implementation/handoff.md): там кнопка «Искать по словам сейчас» уже слала `wait_models: false`, а воркер поле не знал (OQ-280).

## Сделано

- **`wait_models: Option<bool>` в `SearchParams`** (`crates/bars-protocol/src/methods.rs`): нет поля — ждать, как раньше; `false` — не ждать. Поле необязательное и в запрос без него не попадает.
- **Воркер** (`crates/bars-worker/src/handler/mod.rs`, `search_with_graph`): при `Some(false)` предел ожидания нулевой. Загружающийся embed не ждётся, выдача идёт без смысловой части с тем же `semantic_error`, что при истечении предела; загружающийся rerank пропускается с `rerank_error`, `score_kind` — `none`. Ошибка загрузки, `reindex_required` и скачивание модели (`percent < 100`, `model_loading`) остаются как были. Загруженные модели поле не затрагивает.
- **Литералы `SearchParams`** — `wait_models: None` в 11 литералах: `bars-cli`, `bars-mcp`, `handler/topics.rs`, `handler/tests.rs`, `handler/tests/history.rs`, четыре примера `bars-worker/examples`, два теста протокола. MCP и CLI поле не отдают: агентам нужен весь предел (ISS-105).
- **Тесты, не запускались:** протокол — `search_wait_models_defaults_to_waiting_and_reads_false`, круговой тест и тест пропуска пустых полей дополнены; воркер, `handler/tests/model_wait.rs` — `wait_models_false_answers_at_once_by_text_with_the_same_note`, `wait_models_false_marks_both_unloaded_models_as_skipped_without_waiting`, `wait_models_false_does_not_degrade_a_search_with_loaded_models`.
- **Тест ISS-59** возвращён как `ui/src/lib/labels-window.test.ts` на новой карточке (`SearchPane` + `ResultCard`): название, папка и раздел словами, путь только в подсказке (`title`); разбор баллов называет сигналы словами; пометка «только по словам запроса» не показывает код причины; без опоры оценка — прочерк с подсказкой. Список запрещённых слов — `include_related`, `score_kind`, `semantic_error`, `chunk_id`, `mix`, `hybrid`, `cosine`, `fulltext`, `exact`, `rerank` (кроме пояснения «оценка модели (rerank)» в разборе баллов).
- **Документы:** снят оборот «пока кнопка ещё ждёт» в [дизайне глобального поиска](../../features/window/global-search-design.md) и [руководстве](../../manual/search.md); параметр описан в [гибридном поиске](../../features/search/hybrid-search.md) («Ожидание моделей») и [протоколе](../../architecture/protocol.md).

- **Ревью (sonnet, чтением):** блокирующих замечаний нет; шесть незначительных — пять учтены (формулировка про rerank в «Ожидании моделей», строка `model_loading` в протоколе, имя второго теста, убрана пометка статуса из дизайна окна, пометка шага в плане).

## Не сделано

- Второй тест воркера не проверяет ветку «embed готов, rerank грузится при нулевом пределе» отдельно: embed в нём тоже выгружен. Для неё нужен тест с прогретым embed.
- `retrySearchLater` в `app.svelte.ts` повторяет поиск без `wordsOnly`: после скачивания модели повтор снова ждёт до 20 с (не дефект шага).

- Ничего не собиралось и не запускалось: `cargo`, `check.ps1`, `npm`, `vitest`. Окно не открывалось.
- Возможные места первой правки: `Env::search_now` и порог 1,5 с в трёх новых тестах `model_wait.rs` (на медленной машине порог может быть тесным), `\b` в `CODES` теста ISS-59 (границы слов работают только для латиницы, это намеренно), селекторы `.dpath` и `.score[title=…]`.
- При `false` и скачивании модели (`percent < 100`) воркер по-прежнему отвечает `model_loading`, и окно повторяет поиск само; это следует из [ожидания моделей](../../features/search/hybrid-search.md) («Без ожидания остаются»), а не из OQ-280, и в документах оговорено.

## Дальше

Автор разрешает сборку → `check.ps1`, `vitest` → правки по ошибкам; после проверки — приёмка в окне: загрузить выгруженную модель, нажать «Искать по словам сейчас» и убедиться, что выдача приходит сразу с жёлтым предупреждением.
