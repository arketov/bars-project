# Стенд этапа 11

Методика — `docs/features/models/inference-plan.md`. Rust example `bench_inference`, анализатор `scripts/analyze-inference.mjs`. Экспериментальные параметры не читаются продуктом.

Продуктовый `BuiltinReranker` внедряет отдельно проверенный FP16 GPU loader; `production.json` выбирает именно его. CPU остаётся FP32. Отчёт и границы аппаратного переноса — `docs/knowledge/inference-speed.md`.

Приёмка loader: `seed-product-cache.mjs` создаёт свою копию кэша; `product-source-state.mjs capture` фиксирует входы до release build, без аргументов сверяет после build. Режимы `full` на overdrive/bars/holdout и `worker` выполняются последовательно с новыми output-root, `QualityOnly` и при необходимости `MinimumCommitGiB=9` (резерв проверенного FP16 пути, не минимальное требование продукта). После прогона `product-source-state.mjs report` записывает дрейф общих исходников; `accept-product.mjs <prefix>` сверяет 138 core + 22 IPC с FP32 и сохраняет обезличенные строки. Результаты принадлежат испытанной сборке, не последующим параллельным правкам. `preserve-final.mjs` сохраняет сводки и SHA256; `check-docs.mjs` проверяет ссылки/оглавление/хеши. Не архивировать целиком worker/index output: там тексты и SQLite чужого проекта.

```text
cargo build --release --offline -p bars-core --example bench_inference
bench_inference prepare <snapshot-stage9> <cases.json> <models> <new-data-dir>
bench_inference run <data> <models> <config.json> <new-run-dir>
bench_inference profile <data> <models> <config.json> <model.onnx> <new-profile-dir>
bench_inference full <data> <models> <config.json> <new-full-dir>
node scripts/analyze-inference.mjs <data>/inputs.json <summary.json> <run-dir> [...]
```

`prepare` проверяет хеши снимка этапа 9, валидирует опоры, копирует SQLite и фиксирует первые 15 гибридных кандидатов C30. `run` проверяет хеш входа; каждый запуск требует новую папку. Нет автоматического CPU fallback при отказе регистрации DirectML. Размещение отдельных узлов проверяется отдельно через профиль.

`run` включает диагностическое вычисление токенов и обратную перестановку в latency; абсолютное время не равно времени нынешнего продукта. `full` с `name=production` вызывает штатный BuiltinReranker; варианты включают стоимость своей подготовки, поэтому итоговое сравнение проверяет реальную пользу несмотря на диагностику. Прогрев/загрузка записаны отдельно, времена профиля не участвуют в выборе победителя.

`configs/` замораживает параметры до запусков. Десять screen-вопросов: первые два train-вопроса каждой категории, без выбора по рангу/скорости. Основной test уже просмотрен в этапе 9, здесь регрессия. При выборе усечения/кванта/новой модели нужен новый отложенный набор; эквивалентные настройки проверяются на всех существующих вопросах.

Каталоги `data`, `runs`, `provenance` локальные и исключены из Git: содержат полные тексты другого проекта, модели и тяжёлые профили. Компактные доказательства результата — в `docs/knowledge/`.

`run-series.ps1`: перед запуском 20 секунд без других бенчей/rustc/cargo, RAM ≥8 ГБ и свободный commit ≥9 ГБ (`run`) или ≥12 ГБ (остальные режимы). Preflight ограничен 120 секундами (`-PreflightTimeoutSeconds`); при нехватке памяти модель не запускается. Во время опыта RAM <2 ГБ или commit headroom <4 ГБ прерывают только собственный процесс и всю оставшуюся серию, запуск помечается invalidated. Исключение монитора тоже освобождает собственный дочерний процесс. Для финальных прогонов монитор пишет dedicated/shared GPU memory по точному PID; отсутствие счётчика означает отсутствие данных, не нулевое потребление. `-QualityOnly` разрешает фоновые сборки и принудительно помечает время загрязнённым. `preserve-initial.mjs` сохраняет предварительные исходные прогоны и контрольные суммы; это не упаковщик финальных результатов.
