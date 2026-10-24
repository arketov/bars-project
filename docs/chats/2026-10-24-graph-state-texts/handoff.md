# Тексты состояний графа — вместо отладочного вывода Rust (шаг 2.1.2)

## Сделано
- Предупреждение о неполной близости чанков: «Близость посчитана для 539 из 1856 чанков: достигнут предел сравнений» (`similarity_partial_warning`, `StopReason::label`); слои находят его по `is_similarity_warning`, а не по префиксу в двух местах (`bars-core/src/graph/build.rs`, `bars-worker/src/handler/graph.rs`).
- Причины «граф ещё не готов» словами (`LayerState::not_ready_reason`): вместо `Stale`, `stale`, `not_built`, `method_changed`, `semantic_not_built`; предупреждение о прежнем снимке при `building`/`cancelled`/`failed` — `LayerState::background_job_warning`.
- Графы, построенные раньше, не перестраиваются: `store::load` переводит старое предупреждение («Граф близости частичный: Some(ComparisonLimit); завершено 539/1856 строк») в новый вид при чтении (`humanize_warning`).
- Документы: [архитектура графа](../../architecture/graph.md), `issues.md` (ISS-83 исправлена), `CHANGELOG.md`.
- Тесты: два новых в `bars-graph/tests/storage.rs` (причины и предупреждения без `Debug`, перевод старого текста); текст ошибки в `PhysicalGraph.test.ts` приведён к новому.

## Не сделано
- Заголовок текстового ответа MCP «=== GRAPH: Relations ===» и JSON в блоке остаются: это ISS-22, шаг 4.3.4.
- Состояние `stale`/`partial` как отдельные признаки свежести и полноты — этап 2.2 (ISS-81, ISS-86).

## Трудности
- `cargo fmt --check` по workspace красный из-за чужого неотслеживаемого файла другой сессии `crates/bars-core/examples/quality/registry.rs`; остальные шаги `scripts/check.ps1` прогнаны вручную, свои файлы отформатированы.

## Дальше
- Шаг 2.1.3: заглушка графа строится из документов заглушки, сцена «нужна переиндексация» (ISS-53).
