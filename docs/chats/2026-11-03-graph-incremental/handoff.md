---
title: Handoff — частичное обновление графа, шаг 1.1.4, 03-11-2026
---

# Handoff — частичное обновление графа (шаг 1.1.4, НФТ-7)

Продолжение [handoff проекта 1.1.3](../2026-11-03-graph-incremental-design/handoff.md) и [дешёвой публикации 1.1.2](../2026-11-03-graph-cheap-publish/handoff.md).

## Сделано

- Документы: в [архитектуре графа](../../architecture/graph.md) схема графа 3, таблица `graph_parts`, раздел «Реализация шага 1.1.4 и отличия от проекта» (точки кода, порядок транзакции дельты, отличия), уточнённые разделы «Таблица частей», «Ключ входов, generation, кэш и проверка», «Миграция», «Приёмка» (метод замера дельты и оценка записи) и «Риски». Таблица `document_sources` — в [схеме индекса](../../architecture/schema.md) и [обновлении индекса](../../features/indexing/updates.md). Видимое поведение — в [реализации GraphRAG](../../features/graphrag/implementation.md) и [руководстве](../../manual/graph.md); статус в [исследовании нагрузки](../../knowledge/resource-efficiency.md) и ISS-101, ISS-103, ISS-104: «код готов, проверка ждёт общего прогона».
- Код (не собирался, не запускался; `rustfmt` на новых и изменённых файлах):
  - текст документа в индексе: `crates/bars-core/src/storage/index.rs` (`document_sources`, `DocumentRecord::text`, `Index::document_source`, удаление вместе с документом), `crates/bars-core/src/indexing/indexer.rs` (пишет текст), `indexer/tests.rs`, `examples/bench_quality.rs`, `examples/bench_search_cpu.rs` (`text: None`);
  - полная сборка из индекса: `crates/bars-core/src/graph/build.rs` — текст по цепочке «индекс при равном хеше → прежнее поколение графа → диск со сверкой хеша», `relative_path`, `indexed_source`, `disk_source`; `build/job.rs` — `GraphBuildState::{delta, automatic}`, `config` виден дельте, фикстура пишет текст;
  - разбор и сборка: `crates/bars-graph/src/source.rs` — `parse_document`, `assemble`, `resolve_link`, `parts_for`, `merge_entity`, `PendingLink`, `DocParse`; `model.rs` — `Part`, `Dataset::parts`;
  - схема v3 и дельта: `crates/bars-graph/src/store/schema.rs` (миграция 2 → 3, `graph_parts`, индекс по `evidence_id`, триггеры), `store.rs` (части и `delta_base` в полной публикации, `source_text`, версии 2|3), `store/delta.rs` (`apply_delta`, `delta_base`);
  - подготовка дельты: `crates/bars-core/src/graph/build/delta.rs` (`try_delta`, инкрементальный kNN, решение «дельта или полная сборка»);
  - воркер: `crates/bars-worker/src/worker/graph.rs` — `build_state.delta = !force`.
- Тесты написаны (не запускались): оракул равенства с полной сборкой — правка, добавление, удаление, переименование, общая сущность, вики-ссылки и якоря, непригодная база, текст из индекса вместо диска (`crates/bars-core/src/graph/build/delta.rs`); миграция 2 → 3, части и признак базы (`crates/bars-graph/tests/storage.rs`); `document_sources` (`crates/bars-core/src/storage/index.rs`).

## Не сделано

- Ни сборки, ни тестов, ни `scripts/check.ps1`, ни замера записи на правку (запрет автора); код не закоммичен.
- Не реализовано из проекта: периодический полный `validate` раз в 50 дельт, применение дельты к кэшу читателей, построчная запись `knn_complete`. Откат на каждом шаге транзакции отдельным тестом не покрыт.
- Существующие тесты `bars-worker` на сценарии «правка → граф» и `bars-graph/tests` с порядком evidence у явных рёбер не просматривались полностью: их может задеть сортировка evidence у рёбер правил.

## Сложности и отличия от проекта

- Полная сборка переписана как «разбор документа, затем слияние»; без этого равенство с дельтой нечем доказать. Побочно evidence явных рёбер теперь отсортированы по идентификатору, `broken_links` — по `(путь, номер ссылки)`; это та разовая перестройка, которую допускает OQ-259.
- В части проекта изменено: два вида частей `link`/`wiki`, добавлен индекс `graph_edge_evidence(evidence_id)`; порог «больше четверти» дополнен «и больше 16 документов»; проверка дельты упрощена; ключ входов — счётчик `knn_input_revision` в транзакции; `comparisons` близости дельты равно полному числу пар. Полный список — в разделе «Реализация шага 1.1.4 и отличия от проекта».
- Проверки на сеансе не делались, поэтому возможны ошибки компиляции (заимствования в `apply_delta`, замыкание якорей, структурное обновление `GraphBuildState`) и расхождения с оракулом; первое, что нужно смотреть в общем прогоне — `crates/bars-graph/src/store/delta.rs` и `crates/bars-core/src/graph/build/delta.rs`.

## Дальше

- После разрешения автора: `scripts/check.ps1`; затем замер записи на правку (медианный и крупнейший документ overdrive-bots) по методу в разделе «Приёмка по НФТ-7» и запись результата в ISS-101 и исследование нагрузки.
- Оценка записи на правку среднего документа overdrive-bots (не замер): порядка 6–8 МБ в WAL, 12–16 МБ с переносом в файл БД; чтение порядка 25–30 МБ без чтения документов с диска.
- Незакоммиченные файлы кода этого шага — список выше; чужой незакоммиченный код в тех же файлах (`store.rs`, `schema.rs`, `build.rs`, `worker/graph.rs`, `tests/storage.rs`) принадлежит шагам 1.1.2, 1.1.5, 2.2.1, 3.1.1, 3.1.2.
