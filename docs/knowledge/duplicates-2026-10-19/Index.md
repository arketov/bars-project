---
title: Сырые данные исследования повторов для get_duplicates
---

# Сырые данные исследования повторов для get_duplicates

← База знаний (`knowledge/Index.md`) · Выводы (`knowledge/duplicates.md`)

Снимок 19-10-2026. Воспроизведение — `scripts/duplicates-probe.mjs`: `make` строит размеченный набор, `export` выгружает чанки и векторы временного проекта, `analyze` считает метрики. Корпус настоящих документов — снимок графа (`knowledge/graph-embedding-2026-10-18/Index.md`). JSON.gz распаковываются стандартным gzip; `vectors.bin.gz` — BGV1 (magic, u32 число, u32 размерность, float32 LE).

| Файл | Когда читать |
|---|---|
| `labels.json` | Разметка набора: файл, группа, вид копии, общий шаблон |
| `bench/` | Чанки (`snapshot.json.gz`) и векторы bge-m3 (`vectors.bin.gz`) 94 разделов набора |
| `analysis/` (summary.json) | Распределения `cos`, `c5`, `j5`, `c3`, `cw` по классам |
| `analysis/sweep.json` | Перебор порогов, подавление шаблонов, признаки различия чисел и отрицаний |
| `analysis/bench-pairs.json` | Метрики всех 4 371 пар набора |
| `analysis/corpus-candidates.json` | 114 пар настоящих 569 чанков с общим текстом или косинусом ≥ 0,85 |
| `analysis/latency.json` | Задержка поиска кандидатов на 569, 5 000, 20 000 чанках и полного перебора векторов |
