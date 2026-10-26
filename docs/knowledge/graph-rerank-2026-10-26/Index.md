---
title: Парный прогон GraphRAG с rerank 26-10-2026 — данные и повтор
---

# Парный прогон GraphRAG с rerank 26-10-2026 — данные и повтор

Статус: выполнены 36 вопросов × 10 вариантов, 216 настоящих rerank-вызовов без fallback поиска. Все S1–S8 сохранены. Известный корпус 20-10-2026 — повтор, не новый независимый holdout. Выводы принадлежат `knowledge/graph-research.md`; автор сохранил режим как явно включаемый экспериментальный контекст (OQ-199).

## Материалы парного прогона

| Файл | Когда читать |
|---|---|
| `Index.md` | Метод, воспроизведение и границы |
| `dataset.json.gz` | Пересобранный текущим кодом граф: 66 прежних источников, 2582 точных spans, 15 явных отношений, полный kNN |
| `corpus.json.gz` | Ревизии, исключения, snapshot и время построения |
| `results.json.gz` | 360 первичных выдач, применённые параметры, устройство и время |
| `analysis.json.gz` | Пакеты с общим бюджетом 16 KiB, отдельные основные карточки и related, метрики каждого случая |
| `summary.json` | Все S-группы/части набора, парные выигрыши/потери, Hit@3 контролей и описательные времена |
| `provenance.json` | SHA-256 входов, исходников, exe и результатов, базовый HEAD и ограничения |

## Воспроизведение парного прогона

Из корня репозитория восстановить старые источники/SQLite/векторы в **новую** изолированную папку, например `.bars/repeat-graph-rerank`:

```powershell
node scripts/prepare-graph-rerank.mjs docs/knowledge/graph-quality-2026-10-20 .bars/repeat-graph-rerank
pwsh -NoProfile -File docs/chats/2026-10-26-graph-rerank/run.ps1
C:/bt/bars/debug/examples/graph_quality.exe freeze .bars/repeat-graph-rerank
C:/bt/bars/debug/examples/graph_quality.exe run-rerank .bars/repeat-graph-rerank docs/knowledge/graph-quality-2026-10-20/gold-matched.json C:/Users/arketov/.bars/models .bars/repeat-graph-rerank/results.json
node scripts/graph-quality-analyze.mjs .bars/repeat-graph-rerank docs/knowledge/graph-quality-2026-10-20/gold-matched.json .bars/repeat-graph-rerank/results.json .bars/repeat-graph-rerank/analysis.json.gz
node docs/chats/2026-10-26-graph-rerank/summarize.mjs .bars/repeat-graph-rerank/analysis.json.gz .bars/repeat-graph-rerank/results.json .bars/repeat-graph-rerank/summary.json
```

Кэш сборки только `C:/bt/bars`; скрипт задаёт штатную MSVC/Vulkan/LLVM-среду этой машины. Встроенные веса уже должны находиться в локальном кэше: verified FP16 и FP32 fallback. Проверка наличия предшествует loader; рабочие настройки и `.bars/index.sqlite` проекта не используются. Имена результатов не перезаписываются. Query vectors и корпус берутся из прежнего архива; embeddings заново не считаются. Для точного повтора сохранённого snapshot вместо нового `freeze` распаковать `dataset.json.gz` данного опыта как `dataset.json`: namespace включает путь папки, поэтому новая сборка в другом пути меняет ObjectRef и возможный порядок равных кандидатов.

Пересчитать только оценку можно без весов и exe: нужны распакованные `dataset.json`, `results.json`, прежний `gold-matched.json`, оценщик и `summarize.mjs`. Повтор оценки проверяет SHA-256 исходных текстов gold, точность всех spans и бюджет пакетов. Повтор inference может немного менять оценки FP16; бинарный повтор GPU не обещается.

Проверки 26-10-2026: повтор оценки побайтово совпал (SHA-256 `8e9fad0f7a08c6234f0488e1109c8ddb091c4347bda2fc0d8bfab308a981f4e8`), регрессия оценщика и синтаксис трёх исследовательских скриптов прошли. Полный `scripts/check.ps1 -Jobs 2` с `RUST_TEST_THREADS=1` зелёный во втором прогоне; первый остановился на SQLite-ошибке внешнего worker-теста. Оба лога сохранены в сессии, первый сбой не объявлен исправленным.

## Ограничения парного прогона

Порог и состав вопросов зафиксированы до результатов. Все пары используют одну ревизию графа, один scope, сохранённые query vectors и одинаковый бюджет. Нулевая оценка в основном опыте отделена от штатного порога 0,1 и трёх результатов окна. Семантическая точность неразмеченного текста, нормативный статус решений, полнота ролей/цепочек и тематического обзора из evidence recall не следуют. Четырёх контролей недостаточно для общей границы регрессии 2%.

Стенд вызывает ядро и штатный loader rerank; Worker/IPC, загрузка кэша графа, native/установленное окно не проверялись. Debug, фиксированный порядок и параллельные сборки исключают ресурсную приёмку. Никакая новая модель, переиндексация живого проекта или установка не выполнялись.
