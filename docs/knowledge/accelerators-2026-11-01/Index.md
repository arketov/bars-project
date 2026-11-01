---
title: Ускорители RTX 3090 — доказательства 01-11-2026
---

# Ускорители RTX 3090 — доказательства 01-11-2026

Сырые данные исследования; выводы и границы — в `knowledge/inference-speed.md`. Нативные библиотеки, промежуточные сборки и распакованные runtime в репозиторий не входят.

| Файл | Когда читать |
|---|---|
| `inputs.json` | 24 замороженных вопроса bars и прежние кандидаты; сравнение scores при неизменном retrieval |
| `gguf-inputs.json` | Четыре вопроса и естественные документы для GGUF embedding |
| `fixed-inputs.json` | Те же четыре вопроса, по четыре документа: одинаковая нагрузка fixed/binding/graph |
| `artifacts.json` | Размеры, SHA-256 и происхождение весов и tokenizer |
| `hardware.json` | CPU, RAM, GPU, драйвер и установленная CUDA |
| `os.json` | ОС и номер сборки Windows |
| `runtimes.json` | Закреплённые архивы, DLL, SHA-256 и подписи NVIDIA; версии исследовательских зависимостей |
| `protocol.json` | Конфигурации серий, формат binary-векторов, хеши исходников при архивировании, отказы preflight |
| `summary.json` | Медианы, p95, память, отклонения scores, переходы порога, статусы и размещение GPU-операторов |
| `rows.jsonl.gz` | Сырые события и логиты; векторы заданы смещением и длиной |
| `vectors.f32.gz` | Lossless little-endian f32-векторы; смещение из rows измеряется в элементах, не байтах |
| `monitor.jsonl.gz` | Двухсекундные RAM/commit/private, общая VRAM, CPU; собственный и фоновый CPU доступны у поздних серий |
| `native.jsonl.gz` | Native-основания: offload, flash attention, фактические batch и размер KV |
| `profiles.jsonl.gz` | Исходные Node/Session события ORT для обычного CUDA и CUDA Graphs |
| `checksums.json` | SHA-256 файлов доказательств |
