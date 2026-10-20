---
title: Память — данные приёмки 20-10-2026
---

# Память — данные приёмки 20-10-2026

[← База знаний](../Index.md). Метод, выводы и ограничения — [memory.md](../memory.md#аппаратное-продолжение-20-10-2026). 138 вопросов без новых потерь, один совместный FP16-процесс; второй не допущен по commit. TEMP с весами удалён. Время при фоновой работе не принимается как скорость продукта.

| Файл | Когда читать |
|---|---|
| [20261020T090035448Z-previous-pair-third-auto-bounded-1.gpu.jsonl](20261020T090035448Z-previous-pair-third-auto-bounded-1.gpu.jsonl) | GPU и сопутствующая нагрузка |
| [20261020T090035448Z-previous-pair-third-auto-bounded-1.jsonl](20261020T090035448Z-previous-pair-third-auto-bounded-1.jsonl) | Фазы и нативный сэмплер памяти 100 мс |
| [20261020T090035448Z-previous-pair-third-auto-bounded-1.process.json](20261020T090035448Z-previous-pair-third-auto-bounded-1.process.json) | Код завершения собственного процесса |
| [20261020T090035448Z-previous-pair-third-auto-bounded-1.stderr.txt](20261020T090035448Z-previous-pair-third-auto-bounded-1.stderr.txt) | Диагностика запуска |
| [analyzer-check.json](analyzer-check.json) | Проверки анализатора: завершение и непригодность времени |
| [artifact-initial-preflight.jsonl.gz](artifact-initial-preflight.jsonl.gz) | Передача одного FP16-артефакта и проверка целостности |
| [bench-build.log.gz](bench-build.log.gz) | Сборка исследовательского бинарника и её диагностика |
| [build.json](build.json) | Сборка исследовательского бинарника и её диагностика |
| [cleanup-verification.json](cleanup-verification.json) | Отсутствие собственного TEMP и сохранность общих каталогов моделей |
| [clients-after.json](clients-after.json) | Целостность известных файлов клиентских конфигураций |
| [clients-before.json](clients-before.json) | Целостность известных файлов клиентских конфигураций |
| [control-bars.complete.json.gz](control-bars.complete.json.gz) | Итог прохода, идентичность индекса и контрольные хеши; gzip |
| [control-bars.native-memory.jsonl.gz](control-bars.native-memory.jsonl.gz) | Нативные сэмплы и завершение процесса; gzip |
| [control-overdrive.complete.json.gz](control-overdrive.complete.json.gz) | Итог прохода, идентичность индекса и контрольные хеши; gzip |
| [control-overdrive.native-memory.jsonl.gz](control-overdrive.native-memory.jsonl.gz) | Нативные сэмплы и завершение процесса; gzip |
| [core-memory-summary.json](core-memory-summary.json) | Пики полной индексации и последовательной проверки качества |
| [data-bars-cases.json.gz](data-bars-cases.json.gz) | Замороженные вопросы и эталонные результаты; gzip |
| [data-bars-inputs.json.gz](data-bars-inputs.json.gz) | Замороженные вопросы и эталонные результаты; gzip |
| [data-bars-manifest.json.gz](data-bars-manifest.json.gz) | Происхождение корпуса, вопросов и FP32-контроля; gzip |
| [data-overdrive-cases.json.gz](data-overdrive-cases.json.gz) | Замороженные вопросы и эталонные результаты; gzip |
| [data-overdrive-inputs.json.gz](data-overdrive-inputs.json.gz) | Замороженные вопросы и эталонные результаты; gzip |
| [data-overdrive-manifest.json.gz](data-overdrive-manifest.json.gz) | Происхождение корпуса, вопросов и FP32-контроля; gzip |
| [dependency-sources.json](dependency-sources.json) | Хеши проверенных закреплённых исходников allocator API |
| [download-source.json.gz](download-source.json.gz) | Передача одного FP16-артефакта и проверка целостности |
| [environment.json](environment.json) | Параметры и диагностика исследовательского прохода |
| [example-tests.log.gz](example-tests.log.gz) | Пять тестов ядрового стенда |
| [fixture.json](fixture.json) | Фиксированные 15 документов парного опыта |
| [index-bars-core-fp16-complete.json.gz](index-bars-core-fp16-complete.json.gz) | Итог прохода, идентичность индекса и контрольные хеши; gzip |
| [index-bars-core-fp16-full-data-cases.json.gz](index-bars-core-fp16-full-data-cases.json.gz) | Замороженные вопросы и эталонные результаты; gzip |
| [index-bars-core-fp16-full-data-index.sqlite.gz](index-bars-core-fp16-full-data-index.sqlite.gz) | Свежий FP16-индекс для воспроизведения качества; gzip |
| [index-bars-core-fp16-full-data-inputs.json.gz](index-bars-core-fp16-full-data-inputs.json.gz) | Замороженные вопросы и эталонные результаты; gzip |
| [index-bars-core-fp16-full-data-manifest.json.gz](index-bars-core-fp16-full-data-manifest.json.gz) | Происхождение корпуса, вопросов и FP32-контроля; gzip |
| [index-bars-core-fp16-native-memory.jsonl.gz](index-bars-core-fp16-native-memory.jsonl.gz) | Нативные сэмплы и завершение процесса; gzip |
| [index-bars-core-fp16.monitor.json.gz](index-bars-core-fp16.monitor.json.gz) | Предусловия, запас памяти и exit code процесса; gzip |
| [index-bars-core-fp16.stderr.log.gz](index-bars-core-fp16.stderr.log.gz) | Параметры и диагностика исследовательского прохода |
| [index-bars-core-fp16.stdout.log.gz](index-bars-core-fp16.stdout.log.gz) | Параметры и диагностика исследовательского прохода |
| [index-bars-preflight-core-fp16.jsonl.gz](index-bars-preflight-core-fp16.jsonl.gz) | Параметры и диагностика исследовательского прохода |
| [index-overdrive-core-fp16-complete.json.gz](index-overdrive-core-fp16-complete.json.gz) | Итог прохода, идентичность индекса и контрольные хеши; gzip |
| [index-overdrive-core-fp16-full-data-cases.json.gz](index-overdrive-core-fp16-full-data-cases.json.gz) | Замороженные вопросы и эталонные результаты; gzip |
| [index-overdrive-core-fp16-full-data-index.sqlite.gz](index-overdrive-core-fp16-full-data-index.sqlite.gz) | Свежий FP16-индекс для воспроизведения качества; gzip |
| [index-overdrive-core-fp16-full-data-inputs.json.gz](index-overdrive-core-fp16-full-data-inputs.json.gz) | Замороженные вопросы и эталонные результаты; gzip |
| [index-overdrive-core-fp16-full-data-manifest.json.gz](index-overdrive-core-fp16-full-data-manifest.json.gz) | Происхождение корпуса, вопросов и FP32-контроля; gzip |
| [index-overdrive-core-fp16-native-memory.jsonl.gz](index-overdrive-core-fp16-native-memory.jsonl.gz) | Нативные сэмплы и завершение процесса; gzip |
| [index-overdrive-core-fp16.monitor.json.gz](index-overdrive-core-fp16.monitor.json.gz) | Предусловия, запас памяти и exit code процесса; gzip |
| [index-overdrive-core-fp16.stderr.log.gz](index-overdrive-core-fp16.stderr.log.gz) | Параметры и диагностика исследовательского прохода |
| [index-overdrive-core-fp16.stdout.log.gz](index-overdrive-core-fp16.stdout.log.gz) | Параметры и диагностика исследовательского прохода |
| [index-overdrive-preflight-core-fp16.jsonl.gz](index-overdrive-preflight-core-fp16.jsonl.gz) | Параметры и диагностика исследовательского прохода |
| [indexer-control.json](indexer-control.json) | Параметры и диагностика исследовательского прохода |
| [pair-20261020T101422059Z-both-fp16-auto-bounded-1.gpu.jsonl.gz](pair-20261020T101422059Z-both-fp16-auto-bounded-1.gpu.jsonl.gz) | Совместный FP16 worker: параметры, память, GPU, предусловия и завершение; gzip |
| [pair-20261020T101422059Z-both-fp16-auto-bounded-1.jsonl.gz](pair-20261020T101422059Z-both-fp16-auto-bounded-1.jsonl.gz) | Совместный FP16 worker: параметры, память, GPU, предусловия и завершение; gzip |
| [pair-20261020T101422059Z-both-fp16-auto-bounded-1.process.json.gz](pair-20261020T101422059Z-both-fp16-auto-bounded-1.process.json.gz) | Совместный FP16 worker: параметры, память, GPU, предусловия и завершение; gzip |
| [pair-20261020T101422059Z-both-fp16-auto-bounded-1.stderr.txt.gz](pair-20261020T101422059Z-both-fp16-auto-bounded-1.stderr.txt.gz) | Совместный FP16 worker: параметры, память, GPU, предусловия и завершение; gzip |
| [pair-fixture.json.gz](pair-fixture.json.gz) | Совместный FP16 worker: параметры, память, GPU, предусловия и завершение; gzip |
| [pair-preflight-20261020T101422059Z.jsonl.gz](pair-preflight-20261020T101422059Z.jsonl.gz) | Совместный FP16 worker: параметры, память, GPU, предусловия и завершение; gzip |
| [pair-run-20261020T101422059Z.json.gz](pair-run-20261020T101422059Z.json.gz) | Совместный FP16 worker: параметры, память, GPU, предусловия и завершение; gzip |
| [precision-analyzer-check.json](precision-analyzer-check.json) | Проверки анализатора: полный проход и отказ на повреждённой приёмке |
| [precision-summary.json](precision-summary.json) | Качество на свежих FP16-индексах: 24 + 100 + 14 вопросов |
| [preflight-20261020T085944563Z.jsonl](preflight-20261020T085944563Z.jsonl) | Снимки запаса RAM/commit; отказ не является модельным прогоном |
| [preflight-20261020T085954104Z.jsonl](preflight-20261020T085954104Z.jsonl) | Снимки запаса RAM/commit; отказ не является модельным прогоном |
| [preflight-20261020T090035448Z.jsonl](preflight-20261020T090035448Z.jsonl) | Снимки запаса RAM/commit; отказ не является модельным прогоном |
| [preflight-20261020T090141310Z.jsonl](preflight-20261020T090141310Z.jsonl) | Снимки запаса RAM/commit; отказ не является модельным прогоном |
| [preflight-20261020T090626382Z.jsonl](preflight-20261020T090626382Z.jsonl) | Снимки запаса RAM/commit; отказ не является модельным прогоном |
| [quality-bars-preflight-production.jsonl.gz](quality-bars-preflight-production.jsonl.gz) | Параметры и диагностика исследовательского прохода |
| [quality-bars-production-complete.json.gz](quality-bars-production-complete.json.gz) | Итог прохода, идентичность индекса и контрольные хеши; gzip |
| [quality-bars-production-config.json.gz](quality-bars-production-config.json.gz) | Параметры и диагностика исследовательского прохода |
| [quality-bars-production-native-memory.jsonl.gz](quality-bars-production-native-memory.jsonl.gz) | Нативные сэмплы и завершение процесса; gzip |
| [quality-bars-production-query-vectors.json.gz](quality-bars-production-query-vectors.json.gz) | Параметры и диагностика исследовательского прохода |
| [quality-bars-production-rows.jsonl.gz](quality-bars-production-rows.jsonl.gz) | Полные результаты поисковых вопросов; gzip |
| [quality-bars-production-session.json.gz](quality-bars-production-session.json.gz) | Фактические устройства и precision моделей; gzip |
| [quality-bars-production.monitor.json.gz](quality-bars-production.monitor.json.gz) | Предусловия, запас памяти и exit code процесса; gzip |
| [quality-bars-production.stderr.log.gz](quality-bars-production.stderr.log.gz) | Параметры и диагностика исследовательского прохода |
| [quality-bars-production.stdout.log.gz](quality-bars-production.stdout.log.gz) | Параметры и диагностика исследовательского прохода |
| [quality-overdrive-preflight-production.jsonl.gz](quality-overdrive-preflight-production.jsonl.gz) | Параметры и диагностика исследовательского прохода |
| [quality-overdrive-production-complete.json.gz](quality-overdrive-production-complete.json.gz) | Итог прохода, идентичность индекса и контрольные хеши; gzip |
| [quality-overdrive-production-config.json.gz](quality-overdrive-production-config.json.gz) | Параметры и диагностика исследовательского прохода |
| [quality-overdrive-production-native-memory.jsonl.gz](quality-overdrive-production-native-memory.jsonl.gz) | Нативные сэмплы и завершение процесса; gzip |
| [quality-overdrive-production-query-vectors.json.gz](quality-overdrive-production-query-vectors.json.gz) | Параметры и диагностика исследовательского прохода |
| [quality-overdrive-production-rows.jsonl.gz](quality-overdrive-production-rows.jsonl.gz) | Полные результаты поисковых вопросов; gzip |
| [quality-overdrive-production-session.json.gz](quality-overdrive-production-session.json.gz) | Фактические устройства и precision моделей; gzip |
| [quality-overdrive-production.monitor.json.gz](quality-overdrive-production.monitor.json.gz) | Предусловия, запас памяти и exit code процесса; gzip |
| [quality-overdrive-production.stderr.log.gz](quality-overdrive-production.stderr.log.gz) | Параметры и диагностика исследовательского прохода |
| [quality-overdrive-production.stdout.log.gz](quality-overdrive-production.stdout.log.gz) | Параметры и диагностика исследовательского прохода |
| [range-download.log.gz](range-download.log.gz) | Передача одного FP16-артефакта и проверка целостности |
| [rebuild-core.log](rebuild-core.log) | Сборка исследовательского бинарника и её диагностика |
| [recovered-core-build.json](recovered-core-build.json) | Сборка исследовательского бинарника и её диагностика |
| [recovered-example-tests.log](recovered-example-tests.log) | Пять тестов ядрового стенда |
| [recovered-worker-build.log](recovered-worker-build.log) | Сборка исследовательского бинарника и её диагностика |
| [resource-after-preflight.json](resource-after-preflight.json) | Параметры и диагностика исследовательского прохода |
| [run-20261020T085944563Z.json](run-20261020T085944563Z.json) | Параметры запуска, хеши бинарника и исходников |
| [run-20261020T085954104Z.json](run-20261020T085954104Z.json) | Параметры запуска, хеши бинарника и исходников |
| [run-20261020T090035448Z.json](run-20261020T090035448Z.json) | Параметры запуска, хеши бинарника и исходников |
| [run-20261020T090141310Z.json](run-20261020T090141310Z.json) | Параметры запуска, хеши бинарника и исходников |
| [run-20261020T090626382Z.json](run-20261020T090626382Z.json) | Параметры запуска, хеши бинарника и исходников |
| [serial-smoke.json](serial-smoke.json) | Параметры и диагностика исследовательского прохода |
| [summary.json](summary.json) | Проверенная сводка завершённых парных прогонов |
| [temporary-artifact.jsonl.gz](temporary-artifact.jsonl.gz) | Передача одного FP16-артефакта и проверка целостности |
| [temporary-download.log.gz](temporary-download.log.gz) | Передача одного FP16-артефакта и проверка целостности |
| [temporary-first-transfer.json](temporary-first-transfer.json) | Параметры и диагностика исследовательского прохода |
| [temporary-native-initial-preflight.json](temporary-native-initial-preflight.json) | Параметры и диагностика исследовательского прохода |
| [temporary-native-transfer.json](temporary-native-transfer.json) | Параметры и диагностика исследовательского прохода |
| [temporary-pipeline.json](temporary-pipeline.json) | Частичное завершение: второй парный повтор не стартовал; TEMP удалён |
| [temporary-workspace.json](temporary-workspace.json) | Параметры и диагностика исследовательского прохода |
| [window-background.json](window-background.json) | Пустое изолированное окно и WebView |
| [window-build.json](window-build.json) | Пустое изолированное окно и WebView |
| [window-build.log.gz](window-build.log.gz) | Сборка исследовательского бинарника и её диагностика |
| [window-visible.json](window-visible.json) | Пустое изолированное окно и WebView |
