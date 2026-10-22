---
title: Доказательства опыта графа на существующих embeddings
---

# Доказательства опыта графа на существующих embeddings

← База знаний (`knowledge/Index.md`) · Результат и границы (`knowledge/graph-embedding-2026-10-18.md`)

Снимок P0/development от 18-10-2026, не независимая приёмка GraphRAG. Графы построены без новой модели, только по сохранённым векторам. JSON.gz распаковываются стандартным gzip; `vectors.bin.gz` после распаковки — BGV1: magic, u32 count, u32 dimension, затем float32 LE.

| Файл | Когда читать |
|---|---|
| `manifest.json` | Корпус, исключённые файлы, fingerprint, хеши документов и snapshot/vector payload |
| `snapshot.json.gz` | Неизменные тексты/диапазоны чанков и привязка локальных индексов |
| `vectors.bin.gz` | Исходные готовые векторы для воспроизведения Rust kNN |
| `baselines.json.gz` | Восемь вопросов S1–S8 и фактические ответы MCP с applied parameters |
| `graphs.json.gz` | Девять полных kNN-результатов; ключи — имена входов анализатора |
| `analysis.json` | Baseline/expansion на одинаковом текстовом бюджете; добавленные/вытесненные документы |
| `provenance.json` | Хеши исходников/бинарника, toolchain/CPU, команды, тесты и границы доказательств |
| `cargo-test.log` | Успешная проверка семи тестов графа в составе bars-core release; 207 тестов не запускались |
