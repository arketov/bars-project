---
title: Спайк этапа 22 — Qwen GGUF
---

# Спайк этапа 22 — Qwen GGUF

Изолированный workspace, настоящий bars-core/ORT и закреплённый llama-cpp-2. Результаты и источники — [исследование](../../docs/knowledge/gguf-models.md), критерии — [проект](../../docs/features/models/gguf-plan.md). Это не продуктовый loader.

## Сборка спайка GGUF на Windows

Rust из toolchain проекта, MSVC, CMake. Для Vulkan — SDK LunarG и драйвер GPU. Короткий `CARGO_TARGET_DIR` и `CMAKE_BUILD_PARALLEL_LEVEL=1` нужны из-за вложенной сборки shader generator; одного `CARGO_BUILD_JOBS` недостаточно.

```powershell
$env:PATH = "$env:USERPROFILE/.cargo/bin;$env:PATH"
$env:CARGO_TARGET_DIR = 'C:/bg22'
$env:CMAKE_BUILD_PARALLEL_LEVEL = '1'
$env:VULKAN_SDK = '<папка-SDK>'
cargo build --release --manifest-path spikes/gguf/Cargo.toml --bin spike-gguf-vulkan --features vulkan
```

CPU без SDK: `--bin spike-gguf` без `--features vulkan`. `gpu` в CPU-бинарнике или без доступного GPU — ошибка, молчаливой подмены нет.

## Пробы GGUF

```powershell
'["поиск документации","другой текст","поиск документации",""]' | C:/bg22/release/spike-gguf-vulkan.exe <model.gguf> gpu
C:/bg22/release/spike-gguf-vulkan.exe coexist <model.gguf> gpu <папка-моделей-bars>
```

JSONL stdout содержит вектор, число токенов и время; stderr — загрузку и диагностику backend. `coexist` считает Qwen до и после реальных ONNX embed/rerank в том же процессе. Сравнение векторов обязательно, сам код выхода не доказывает независимость.

## Бенч GGUF

`bench <model.gguf> <cpu|gpu> <frozen-project> <cases.json> <output>` строит отдельный индекс; `bge` вместо устройства загружает bge-m3 из первого аргумента, который тогда означает каталог моделей. Три повтора настоящего search C30 без rerank. `run.ps1` оборачивает вызов, пишет память каждые 0,5 с, пишет память процесса; проверка резерва RAM/commit включается только явным `-EnforceReserve`. Обычный прогон использует выделение памяти ОС и её файл подкачки.

Не использовать живой проект в качестве корпуса. Нужен замороженный snapshot этапа 9 и соответствующий cases.json. `config.json` записывает хеши набора/бинарника, загрузку и индексацию. Возобновление незавершённого индекса разрешено и отмечается `resumed_index`; завершённые config/rows не перезаписываются. Результат прерванного прогона невалиден. Время остатка индексации не является временем полного построения.

```powershell
node spikes/gguf/analyze.mjs <cases.json> <output-bge> <output-qwen>
```

`download.ps1` скачивает закреплённый URL диапазонами и проверяет полную длину/SHA-256; готовый destination никогда не перезаписывается. Части и модели держать вне репозитория. Результаты, индексы и локальные логи — в игнорируемом `target/` либо вне репозитория.

Продуктовый loader проверяет `cargo build --release -p bars-core --example verify_gguf` (то же окружение Ninja/SDK). `verify_gguf product <model-id> <models-cache> <cpu|gpu>` читает JSON текстов; четвёртый аргумент с каталогом ORT-моделей добавляет настоящий rerank между повторными Qwen-вызовами. `bench <model-id> <prod-cpu|prod-gpu> <frozen-project> <cases> <output> <models-cache>` использует продуктовый backend; `run.ps1` передаёт каталог через `-ModelsCache`. Продуктовый поиск проверен по 300 строкам каждой модели.