---
title: Память — данные приёмки 20-10-2026
---

# Память — данные приёмки 20-10-2026

← База знаний (`knowledge/Index.md`). Метод, выводы и ограничения — `memory.md`. 138 вопросов без новых потерь, один совместный FP16-процесс; второй не допущен по commit. TEMP с весами удалён. Время при фоновой работе не принимается как скорость продукта.

| Файл | Когда читать |
|---|---|
| `20261020T090035448Z-previous-pair-third-auto-bounded-1.gpu.jsonl` | GPU и сопутствующая нагрузка |
| `20261020T090035448Z-previous-pair-third-auto-bounded-1.jsonl` | Фазы и нативный сэмплер памяти 100 мс |
| `20261020T090035448Z-previous-pair-third-auto-bounded-1.process.json` | Код завершения собственного процесса |
| `20261020T090035448Z-previous-pair-third-auto-bounded-1.stderr.txt` | Диагностика запуска |
| `analyzer-check.json` | Проверки анализатора: завершение и непригодность времени |
| `artifact-initial-preflight.jsonl.gz` | Передача одного FP16-артефакта и проверка целостности |
| `bench-build.log.gz` | Сборка исследовательского бинарника и её диагностика |
| `build.json` | Сборка исследовательского бинарника и её диагностика |
| `cleanup-verification.json` | Отсутствие собственного TEMP и сохранность общих каталогов моделей |
| `clients-after.json` | Целостность известных файлов клиентских конфигураций |
| `clients-before.json` | Целостность известных файлов клиентских конфигураций |
| `control-bars.complete.json.gz` | Итог прохода, идентичность индекса и контрольные хеши; gzip |
| `control-bars.native-memory.jsonl.gz` | Нативные сэмплы и завершение процесса; gzip |
| `control-overdrive.complete.json.gz` | Итог прохода, идентичность индекса и контрольные хеши; gzip |
| `control-overdrive.native-memory.jsonl.gz` | Нативные сэмплы и завершение процесса; gzip |
| `core-memory-summary.json` | Пики полной индексации и последовательной проверки качества |
| `data-bars-cases.json.gz` | Замороженные вопросы и эталонные результаты; gzip |
| `data-bars-inputs.json.gz` | Замороженные вопросы и эталонные результаты; gzip |
| `data-bars-manifest.json.gz` | Происхождение корпуса, вопросов и FP32-контроля; gzip |
| `data-overdrive-cases.json.gz` | Замороженные вопросы и эталонные результаты; gzip |
| `data-overdrive-inputs.json.gz` | Замороженные вопросы и эталонные результаты; gzip |
| `data-overdrive-manifest.json.gz` | Происхождение корпуса, вопросов и FP32-контроля; gzip |
| `dependency-sources.json` | Хеши проверенных закреплённых исходников allocator API |
| `download-source.json.gz` | Передача одного FP16-артефакта и проверка целостности |
| `environment.json` | Параметры и диагностика исследовательского прохода |
| `example-tests.log.gz` | Пять тестов ядрового стенда |
| `fixture.json` | Фиксированные 15 документов парного опыта |
| `index-bars-core-fp16-complete.json.gz` | Итог прохода, идентичность индекса и контрольные хеши; gzip |
| `index-bars-core-fp16-full-data-cases.json.gz` | Замороженные вопросы и эталонные результаты; gzip |
| `index-bars-core-fp16-full-data-index.sqlite.gz` | Свежий FP16-индекс для воспроизведения качества; gzip |
| `index-bars-core-fp16-full-data-inputs.json.gz` | Замороженные вопросы и эталонные результаты; gzip |
| `index-bars-core-fp16-full-data-manifest.json.gz` | Происхождение корпуса, вопросов и FP32-контроля; gzip |
| `index-bars-core-fp16-native-memory.jsonl.gz` | Нативные сэмплы и завершение процесса; gzip |
| `index-bars-core-fp16.monitor.json.gz` | Предусловия, запас памяти и exit code процесса; gzip |
| `index-bars-core-fp16.stderr.log.gz` | Параметры и диагностика исследовательского прохода |
| `index-bars-core-fp16.stdout.log.gz` | Параметры и диагностика исследовательского прохода |
| `index-bars-preflight-core-fp16.jsonl.gz` | Параметры и диагностика исследовательского прохода |
| `index-overdrive-core-fp16-complete.json.gz` | Итог прохода, идентичность индекса и контрольные хеши; gzip |
| `index-overdrive-core-fp16-full-data-cases.json.gz` | Замороженные вопросы и эталонные результаты; gzip |
| `index-overdrive-core-fp16-full-data-index.sqlite.gz` | Свежий FP16-индекс для воспроизведения качества; gzip |
| `index-overdrive-core-fp16-full-data-inputs.json.gz` | Замороженные вопросы и эталонные результаты; gzip |
| `index-overdrive-core-fp16-full-data-manifest.json.gz` | Происхождение корпуса, вопросов и FP32-контроля; gzip |
| `index-overdrive-core-fp16-native-memory.jsonl.gz` | Нативные сэмплы и завершение процесса; gzip |
| `index-overdrive-core-fp16.monitor.json.gz` | Предусловия, запас памяти и exit code процесса; gzip |
| `index-overdrive-core-fp16.stderr.log.gz` | Параметры и диагностика исследовательского прохода |
| `index-overdrive-core-fp16.stdout.log.gz` | Параметры и диагностика исследовательского прохода |
| `index-overdrive-preflight-core-fp16.jsonl.gz` | Параметры и диагностика исследовательского прохода |
| `indexer-control.json` | Параметры и диагностика исследовательского прохода |
| `pair-20261020T101422059Z-both-fp16-auto-bounded-1.gpu.jsonl.gz` | Совместный FP16 worker: параметры, память, GPU, предусловия и завершение; gzip |
| `pair-20261020T101422059Z-both-fp16-auto-bounded-1.jsonl.gz` | Совместный FP16 worker: параметры, память, GPU, предусловия и завершение; gzip |
| `pair-20261020T101422059Z-both-fp16-auto-bounded-1.process.json.gz` | Совместный FP16 worker: параметры, память, GPU, предусловия и завершение; gzip |
| `pair-20261020T101422059Z-both-fp16-auto-bounded-1.stderr.txt.gz` | Совместный FP16 worker: параметры, память, GPU, предусловия и завершение; gzip |
| `pair-fixture.json.gz` | Совместный FP16 worker: параметры, память, GPU, предусловия и завершение; gzip |
| `pair-preflight-20261020T101422059Z.jsonl.gz` | Совместный FP16 worker: параметры, память, GPU, предусловия и завершение; gzip |
| `pair-run-20261020T101422059Z.json.gz` | Совместный FP16 worker: параметры, память, GPU, предусловия и завершение; gzip |
| `precision-analyzer-check.json` | Проверки анализатора: полный проход и отказ на повреждённой приёмке |
| `precision-summary.json` | Качество на свежих FP16-индексах: 24 + 100 + 14 вопросов |
| `preflight-20261020T085944563Z.jsonl` | Снимки запаса RAM/commit; отказ не является модельным прогоном |
| `preflight-20261020T085954104Z.jsonl` | Снимки запаса RAM/commit; отказ не является модельным прогоном |
| `preflight-20261020T090035448Z.jsonl` | Снимки запаса RAM/commit; отказ не является модельным прогоном |
| `preflight-20261020T090141310Z.jsonl` | Снимки запаса RAM/commit; отказ не является модельным прогоном |
| `preflight-20261020T090626382Z.jsonl` | Снимки запаса RAM/commit; отказ не является модельным прогоном |
| `quality-bars-preflight-production.jsonl.gz` | Параметры и диагностика исследовательского прохода |
| `quality-bars-production-complete.json.gz` | Итог прохода, идентичность индекса и контрольные хеши; gzip |
| `quality-bars-production-config.json.gz` | Параметры и диагностика исследовательского прохода |
| `quality-bars-production-native-memory.jsonl.gz` | Нативные сэмплы и завершение процесса; gzip |
| `quality-bars-production-query-vectors.json.gz` | Параметры и диагностика исследовательского прохода |
| `quality-bars-production-rows.jsonl.gz` | Полные результаты поисковых вопросов; gzip |
| `quality-bars-production-session.json.gz` | Фактические устройства и precision моделей; gzip |
| `quality-bars-production.monitor.json.gz` | Предусловия, запас памяти и exit code процесса; gzip |
| `quality-bars-production.stderr.log.gz` | Параметры и диагностика исследовательского прохода |
| `quality-bars-production.stdout.log.gz` | Параметры и диагностика исследовательского прохода |
| `quality-overdrive-preflight-production.jsonl.gz` | Параметры и диагностика исследовательского прохода |
| `quality-overdrive-production-complete.json.gz` | Итог прохода, идентичность индекса и контрольные хеши; gzip |
| `quality-overdrive-production-config.json.gz` | Параметры и диагностика исследовательского прохода |
| `quality-overdrive-production-native-memory.jsonl.gz` | Нативные сэмплы и завершение процесса; gzip |
| `quality-overdrive-production-query-vectors.json.gz` | Параметры и диагностика исследовательского прохода |
| `quality-overdrive-production-rows.jsonl.gz` | Полные результаты поисковых вопросов; gzip |
| `quality-overdrive-production-session.json.gz` | Фактические устройства и precision моделей; gzip |
| `quality-overdrive-production.monitor.json.gz` | Предусловия, запас памяти и exit code процесса; gzip |
| `quality-overdrive-production.stderr.log.gz` | Параметры и диагностика исследовательского прохода |
| `quality-overdrive-production.stdout.log.gz` | Параметры и диагностика исследовательского прохода |
| `range-download.log.gz` | Передача одного FP16-артефакта и проверка целостности |
| `rebuild-core.log` | Сборка исследовательского бинарника и её диагностика |
| `recovered-core-build.json` | Сборка исследовательского бинарника и её диагностика |
| `recovered-example-tests.log` | Пять тестов ядрового стенда |
| `recovered-worker-build.log` | Сборка исследовательского бинарника и её диагностика |
| `resource-after-preflight.json` | Параметры и диагностика исследовательского прохода |
| `run-20261020T085944563Z.json` | Параметры запуска, хеши бинарника и исходников |
| `run-20261020T085954104Z.json` | Параметры запуска, хеши бинарника и исходников |
| `run-20261020T090035448Z.json` | Параметры запуска, хеши бинарника и исходников |
| `run-20261020T090141310Z.json` | Параметры запуска, хеши бинарника и исходников |
| `run-20261020T090626382Z.json` | Параметры запуска, хеши бинарника и исходников |
| `serial-smoke.json` | Параметры и диагностика исследовательского прохода |
| `summary.json` | Проверенная сводка завершённых парных прогонов |
| `temporary-artifact.jsonl.gz` | Передача одного FP16-артефакта и проверка целостности |
| `temporary-download.log.gz` | Передача одного FP16-артефакта и проверка целостности |
| `temporary-first-transfer.json` | Параметры и диагностика исследовательского прохода |
| `temporary-native-initial-preflight.json` | Параметры и диагностика исследовательского прохода |
| `temporary-native-transfer.json` | Параметры и диагностика исследовательского прохода |
| `temporary-pipeline.json` | Частичное завершение: второй парный повтор не стартовал; TEMP удалён |
| `temporary-workspace.json` | Параметры и диагностика исследовательского прохода |
| `window-background.json` | Пустое изолированное окно и WebView |
| `window-build.json` | Пустое изолированное окно и WebView |
| `window-build.log.gz` | Сборка исследовательского бинарника и её диагностика |
| `window-visible.json` | Пустое изолированное окно и WebView |
