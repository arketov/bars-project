---
title: Сырые данные калибровки порога дедупликации вопросов
---

# Сырые данные калибровки порога дедупликации вопросов

[← База знаний](../Index.md) · [Выводы](../new-topics-threshold-2026-10-19.md)

Снимок 19-10-2026, IDEA-14. Воспроизведение — [scripts/new-topics-threshold.mjs](../../../scripts/new-topics-threshold.mjs) (`prepare`, `analyze`, `checks`, `components`) и пример `crates/bars-core/examples/bench_questions.rs`; команды в разделе «Воспроизведение» выводов. Сырые файлы векторов `auto.json`, `cpu.json`, `fp16-auto.json` (по 4,8 МБ) в репозиторий не вошли; в [vectors-auto.bin.gz](vectors-auto.bin.gz) лежат векторы fp32/DirectML в формате BGV1 (magic, u32 число, u32 размерность, float32 LE) в порядке [items.json](items.json).

| Файл | Когда читать |
|---|---|
| [variants.json](variants.json) | Перефразы, ключевые запросы и ловушки; разметка составлена до просмотра косинусов |
| [items.json](items.json) | Все 224 текста с группой, делением dev/test и видом, и 31 ловушка как пары индексов |
| [analysis.json](analysis.json) | Метрики трёх режимов: шум, распределения, перебор порогов 0,50…0,95, ближайшие разные пары, группировка |
| [components.txt](components.txt) | Какие лексические проверки сколько ловушек ловят и сколько перефразов блокируют |
| [checks.txt](checks.txt) | Перефразы, заблокированные всеми четырьмя компонентами, и ловушки, которые они пропустили |
| [vectors-auto.bin.gz](vectors-auto.bin.gz) | Векторы bge-m3 fp32 DirectML для 224 текстов |

## Контрольные суммы

| Объект | SHA-256 |
|---|---|
| `overdrive.json` | `8d966917556299d5dc9b711cfd053c3921467ef6c70204e1ced0ac9fb5cd1e0f` |
| `variants.json` | `9b8fc0cbe9d66522712e5835ca1088b1f5ce6f1a9f43e885e52267caf49874aa` |
| `bench_questions.exe` (release, из `HEAD e2d869e`) | `07364309498d696b21ad0dfefc9140b998ca672e13e2752e6d786e140b05edce` |
| `texts.json` | `4226af9ca1c518e1d4e7a15ef142e276e5a39153bb18dde1a8808e84918bcf02` |
| `auto.json` (векторы fp32, DirectML) | `a56c7ba54a52c53b52ed8ba8a721d4b7e3804c1072342246c08d307ae07ed345` |
| `cpu.json` (векторы fp32, CPU) | `6a6d661b2a76673c7bbc7636d478f96b16b10539c8fd4404526ceb996beff2c8` |
| `fp16-auto.json` (векторы fp16, DirectML) | `ebc973a421c896e2810afd761539b9c9d7f0c55d325aa9dd6ac642ab673224ec` |

Хеш `new-topics-threshold.mjs` в `analysis.json` (`inputs.script`) относится к версии на момент прогона; дальнейшие правки скрипта меняют его.
