---
title: "GraphRAG P0: граф на готовых векторах"
---

# GraphRAG P0: граф на готовых векторах

Драйвер `knn.rs` компилирует настоящий `bars-core/src/graph/similarity.rs`, без модели и внешних зависимостей. Это development-опыт, не шесть готовых инструментов и не приёмка этапа 33.

Из корня репозитория, Rust 1.99.0 и Node 24:

```powershell
New-Item -ItemType Directory -Force target/stage33-probe | Out-Null
rustc --edition=2024 -O -D warnings spikes/graph/knn.rs -o target/stage33-probe/knn.exe
rustc --edition=2024 --test crates/bars-core/src/graph/similarity.rs -o target/stage33-probe/similarity-tests.exe
./target/stage33-probe/similarity-tests.exe
node --test scripts/graph-embedding-probe.test.mjs
node scripts/graph-embedding-probe.mjs export . target/stage33-probe/new-corpus
./target/stage33-probe/knn.exe target/stage33-probe/new-corpus/vectors.bin 5 0.5 1000000
```

Каталог export должен быть новым. Скрипт открывает существующий `.bars/index.sqlite` только для чтения и снимает одну SQLite-транзакцию; пользовательские настройки/профили не открываются. Экспорт включает corpus/manifest и готовые векторы, исключает журналы и документы исследования графа. Пустые/неодинаковые/неконечные векторы отклоняются, partial не выдаётся за полный граф.

`graph-embedding-probe.mjs analyze <corpus-dir> <baselines.json> <knn.json> [...]` проверяет хеши и сопоставляет MCP hits с замороженными текстами. Схема baseline: массив `{id, query, output}`, где output — фактический JSON `bars_rag_search` с `results` и `applied`; вызов ограничивается `manifest.files`. Нельзя использовать выдачу по всему проекту как matched baseline исключённого корпуса.

Зафиксированные данные — [доказательства](../../docs/knowledge/graph-embedding-2026-10-18/Index.md), метод и результаты — [отчёт](../../docs/knowledge/graph-embedding-2026-10-18.md). Чтобы воспроизвести именно этот опыт, использовать архивные snapshot/vectors/baselines и девять параметров, а не заново экспортированный живой индекс.
