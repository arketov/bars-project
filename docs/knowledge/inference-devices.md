---
title: Устройства, DirectML, CUDA и библиотеки инференса — обзор вариантов
---

# Устройства, DirectML и библиотеки инференса

← База знаний. Обзор первичных источников на 18-10-2026, раздел «CUDA в сборке bars» — на 28-10-2026. Это каталог кандидатов и ограничений, а не решение расширить стек: действующий контракт — Т-3, Т-17; закреплённые зависимости workspace — fastembed `=7.1.1`, ort `=2.0.0-rc.13`, candle-core `0.11`, llama-cpp-2 `0.1.160`. Текущие upstream-функции не считаются доступными этой сборке автоматически. Измерения i7-11700KF / RTX 3090 и выбранный FP16 rerank — [inference-speed.md](inference-speed.md); других аппаратных платформ в замерах нет, рекомендации для них — гипотезы. Какие устройства и precision используются в продукте — [встроенные модели](../features/models/local-models.md).

## Инвентарь железа и возможности исполнения

[Исследование определения железа из Rust](hardware-discovery-2026-11-02.md) проверяет Windows RAM/DXGI/DXCore/D3D12 на RTX 3090 и описывает macOS Metal/Core ML без физической приёмки. Физическая память, shared-предел, бюджет процесса, драйверный CUDA API и установленные Toolkit — разные сведения. Найденная GPU/NPU не доказывает возможность исполнения модели; продуктовый детектор ещё не реализован.

## Выбор устройства зависит от нагрузки

| Устройство | Когда имеет смысл проверить | Основные издержки |
|---|---|---|
| CPU | Одиночный короткий запрос, универсальный baseline | Пропускная способность памяти, конкуренция пулов, ISA и число потоков |
| Встроенная GPU | Индексация и пачки rerank без отдельной VRAM | Общие с CPU память и тепловой бюджет; очереди, синхронизация, размещение буферов |
| Дискретная GPU | Большие матричные вычисления и устойчивый поток пачек | Передача данных, VRAM весов и активаций, запуск kernels, зависимости runtime |
| NPU | Поддерживаемый граф для длительной фоновой работы ради энергопотребления | Компиляция, ограничения форм/операторов/precision, зависимость от поколения и драйвера |

CPU не обязан проигрывать GPU на коротком запросе, NPU не обязан выигрывать по задержке. TOPS разных устройств без одинаковых precision, sparsity и модели время bars не предсказывают. Windows ML описывает NPU как путь для энергоэффективной длительной работы, GPU — для throughput, CPU — как универсальный вариант ([Microsoft: Windows ML](https://learn.microsoft.com/en-us/windows/ai/new-windows-ml/overview)). embed_query, фоновую индексацию и rerank оценивают раздельно; для них действуют независимые настройки устройства ([настройки](../features/settings/settings.md)).

**CPU.** Поддержка ISA у бинарника и наличие быстрых kernels для precision — разные проверки: AVX2/VNNI/BF16 нельзя выводить из слова «Intel» или «AMD». FP32 — контроль, INT8 — кандидат, FP16 без нужного CPU kernel может не исполниться. Candle-модели bars исполняются на CPU в f32 независимо от GPU. Большие веса и активации вытесняют память в paging, и ускорение matmul постоянные page faults не исправит: снижать нужно рабочий набор, число одновременно живущих моделей или precision, а не автоматически увеличивать файл подкачки.

**GPU: покрытие графа важнее флага «GPU включён».** Execution provider принимает поддерживаемые части ONNX-графа, остальные остаются на CPU, а границы дают синхронизации и обмены. Регистрация EP доказывает доступность провайдера, но не исполнение всей модели на нём: сопоставлять нужно профиль узлов, latency и наблюдение устройства ([ORT execution providers](https://onnxruntime.ai/docs/execution-providers/)). У встроенной GPU общая физическая память, но копирования ORT и синхронизация не бесплатны; на ноутбуке рост CPU-потоков уменьшает тепловой бюджет GPU, поэтому режим питания и общую загрузку фиксируют для каждой серии.

## DirectML

DirectML — GPU API поверх DirectX 12. Индекс адаптера следует DXGI: `0` — основной адаптер, не обязательно самый быстрый, и это не индекс NPU. Для DML-сессии ORT нужны `ORT_SEQUENTIAL` и выключенный memory pattern; одновременные Run одной сессии не поддерживаются, разные сессии могут выполнять Run одновременно. Известные при создании формы позволяют больше оптимизаций; динамические размеры можно фиксировать free-dimension overrides по имени или denotation (имена берутся из графа). Страница DML EP указывает opset до 20; совместимость проверяется по реально поставляемой версии ([контракт ORT DirectML](https://onnxruntime.ai/docs/execution-providers/DirectML-ExecutionProvider.html)). DirectML находится в sustained engineering, новые возможности Microsoft развивает в Windows ML: это причина оценить Windows ML при следующем расширении стека, а не отключать рабочий backend ([уведомление](https://github.com/MicrosoftDocs/windows-ai-docs/blob/docs/docs/directml/includes/directml-sustained-engineering-note.md)).

Прежние замеры bars дают разные картины: ранний embed 513 мс/чанк на DML при batch 16 против CPU 430 при batch 4 (пачки разные, процессор был загружен), поздний rerank 15 кандидатов — DML 5,3–5,8 с против CPU 8,5 с ([спайк](spike-search.md)). Сравнение 24 векторов и небольшого набора логитов подтверждает только проверенные артефакты и входы, но не равенство любых CPU/DML результатов, FP16, новых драйверов или другой GPU.

Почему GPU может не дать прироста (рабочие гипотезы, не установленная причина):

| Гипотеза | Как отличить |
|---|---|
| Выбран не тот адаптер или сработал запасной CPU | Записать имя/ID адаптера, фактический EP и причину fallback |
| Граф разделён между CPU и GPU | Профиль узлов, переходы между устройствами и время копирования |
| Слишком много паддинга | Сопоставить реальные длины пар, padded shape и время пачки |
| Малые Run тратят время на запуск и синхронизацию | Отдельно измерить короткий запрос и batch одинаковой длины |
| Неподходящие kernels, precision или экспорт | Тот же ввод на FP32 и FP16, профиль и проверка выходов |
| Давление памяти или питания | Device/shared memory, page faults, частоты и время длительной серии |
| Внешняя задержка — очередь модели | Таймер ожидания отделён от Run; сравнение с поиском без индексации |

Смотреть нужно весь путь, а не график GPU в Task Manager: GPU может простаивать, пока CPU готовит вход ([ORT profiling](https://onnxruntime.ai/docs/performance/tune-performance/profiling-tools.html)). Не переносить dynamic INT8-граф для CPU на DML без проверки покрытия квантованных операторов; не увеличивать число сессий без оценки памяти и p95 — параллелизм на одной GPU не равен росту throughput.

**Диагностика неверных оценок rerank.** Сначала идентифицировать модель: ревизия, граф и все external data с хешами, tokenizer, max_length, имена и типы входов/выходов. Повторить одинаковые пары на CPU и DML, записать сырые логиты до sigmoid и порядок после неё; проверить несколько длин, кириллицу, NaN/Inf. Почти нулевая sigmoid может скрывать крупную ошибку отрицательных логитов. Если артефакт корректен на CPU и ошибочен на DML, область сужается до EP/драйвера/precision, но виновник не определён; переключение на CPU — обход, а не доказательство причины. Существующая диагностика — [local-models.md](../features/models/local-models.md).

## Другие GPU и EP

- **NVIDIA.** Первый кандидат — [CUDA EP](https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html): зависит от совместимых CUDA/cuDNN, что это значит для закреплённой сборки — раздел «CUDA в сборке bars»; CUDA Graphs требуют соблюдения ограничений по формам и адресам. [TensorRT EP](https://onnxruntime.ai/docs/execution-providers/TensorRT-ExecutionProvider.html) — после CUDA baseline: компиляция engine, profiles min/opt/max для динамических форм, cache, VRAM и поставка runtime; выигрыш считают с учётом холодного старта.
- **AMD Windows.** Для широкого набора GPU остаётся кандидатом DirectML; [MIGraphX](https://onnxruntime.ai/docs/execution-providers/MIGraphX-ExecutionProvider.html) требует собственной платформенной матрицы и не универсален.
- **Intel.** Стоит сравнить DirectML и OpenVINO GPU при одинаковых модели, длинах и precision. ORT помечает встроенный OpenVINO EP deprecated и рекомендует [отдельный Plugin EP](https://github.com/onnxruntime/onnxruntime-ep-openvino/blob/main/docs/QuickStart.md): пересборка ORT перестала быть единственным путём, но загрузку плагина в статический runtime bars нужно проверять (версия C API, регистрация, архитектура, native libraries); DLL разных ABI в одном процессе не смешивать.
- **Apple.** [CoreML EP](https://onnxruntime.ai/docs/execution-providers/CoreML-ExecutionProvider.html) позволяет выбирать compute units (CPU, CPU+GPU, CPU+NeuralEngine, ALL); разрешение ANE не гарантирует размещение каждого узла на нём, динамические формы ухудшают скорость. Проверять compute plan и версию macOS; macOS в bars не проверена.

## NPU

Машина исторического спайка — Core Ultra 5 125H (Meteor Lake) с Intel AI Boost. DirectML-адаптер 1 там дал ошибку «недопустимый адаптер»; это не проверка исправности NPU, а DirectML GPU и Intel NPU — разные пути выполнения.

OpenVINO предоставляет Intel NPU backend, но произвольный ONNX поддержанным автоматически не становится. В документации 2026 динамические формы NPU — preview: начиная с NPU40XX/Lunar Lake, bounded shapes, определённые драйверы, Compiler-In-Plugin, проверенное покрытие ограничено vision; ожидать динамический transformer на Meteor Lake не следует ([OpenVINO NPU device](https://docs.openvino.ai/2026/openvino-workflow/running-inference/inference-devices-and-modes/npu-device.html)). Начальный кандидат для 125H — статические batch и sequence length: один граф для коротких запросов и отдельный для rerank, затем проверка компиляции и результатов. Совместимость скомпилированных blobs между версиями OpenVINO не гарантирована; набор buckets увеличивает число сессий и расход памяти. Отказ компиляции — результат проверки совместимости, а не повод засчитать CPU fallback как NPU. Скорость и энергия bge-моделей на NPU неизвестны.

Другие NPU: [Qualcomm QNN EP](https://onnxruntime.ai/docs/execution-providers/QNN-ExecutionProvider.html) использует собственный backend и SDK (для HTP — квантованный граф, калибровка, фиксированные формы) и не путь к Intel NPU. Для AMD NPU нужен поставляемый для конкретного Ryzen AI execution provider ([Windows ML EP mapping](https://learn.microsoft.com/en-us/windows/ai/new-windows-ml/overview)); ни DirectML GPU, ни название «ONNX Runtime» доступ к AMD NPU не подтверждают, совместимость с Rust/ORT bars не проверена.

## Windows ML и поставка ускорителей

Windows ML — слой поставки и выбора hardware EP поверх ONNX Runtime. CPU и DirectML доступны на поддерживаемых им версиях Windows; оптимизированные EP для NPU и отдельных GPU требуют Windows 11 24H2 или новее, и это ограничение нельзя распространять на все установки bars ([требования](https://learn.microsoft.com/en-us/windows/ai/new-windows-ml/overview)). При оценке любого ускорителя проверяют ОС/архитектуру, драйвер, ABI runtime и EP, размер дистрибутива, загрузку зависимостей без сети, лицензии, холодный старт и наблюдаемую причину fallback. Показать устройство в интерфейсе мало: нужны профиль исполнения и правильные выходы.

## Библиотеки и runtime

| Библиотека / runtime | Применение | Цена интеграции и приоритет |
|---|---|---|
| [fastembed-rs](https://github.com/Anush008/fastembed-rs) | Каталог, tokenizer, embed/rerank, ONNX и отдельные candle-модели | Базовый путь; проверять доступность нужных SessionOptions до проектирования настроек |
| [ort](https://docs.rs/ort/2.0.0-rc.13/ort/) + ONNX Runtime | Прямое управление сессиями, EP и тензорами из Rust | Следующий кандидат, если fastembed не открывает настройку; вручную сохранить pooling, normalization, tokenizer и порядок rerank |
| [OpenVINO Runtime](https://docs.openvino.ai/2026/index.html) напрямую | Независимое управление compile_model, устройствами и запросами | C/C++ runtime и Rust FFI; больше собственного кода и отдельный жизненный цикл |
| [Windows ML](https://learn.microsoft.com/en-us/windows/ai/new-windows-ml/overview) | Поставка и выбор аппаратных EP | Перспективный путь Windows; проверить Rust API, EP офлайн, ОС и совместимость с нынешним ORT |
| [Candle](https://github.com/huggingface/candle) | Rust tensor/model framework, CPU/CUDA/Metal; уже используется частью моделей | GPU Candle не означает DirectML/NPU; перенос модели требует проверки kernels, precision и загрузчика |
| [Burn](https://github.com/tracel-ai/burn) | Rust framework с WGPU/CUDA и fusion | Не замена fastembed одной строкой: импорт, операторы, tokenizer и postprocessing |
| [tract](https://github.com/sonos/tract) | Самодостаточный Rust runtime для ONNX, прежде всего CPU | Кандидат облегчённой поставки; bge-графы, shapes и скорость не проверены |
| [Text Embeddings Inference](https://github.com/huggingface/text-embeddings-inference) | Rust-сервер embed/rerank: token-based dynamic batching | Внешний локальный сервер и ориентир для batching; ожидание заполнения пачки может ухудшить одиночную latency; HTTP-контракт проверять отдельно |

Ни один из этих вариантов не принят как новый backend. CUDA/TensorRT, CoreML, QNN и MIGraphX требуют соответствующего оборудования. Burn и tract — резерв при конкретной причине менять runtime.

**GGUF и llama.cpp.** [llama.cpp](https://github.com/ggml-org/llama.cpp) — C/C++ runtime с CPU, Vulkan, CUDA и Metal; GGUF и кванты снижают память, но поддержка архитектуры, pooling и конкретного кванта не гарантирована. Встроенный путь — Rust-обёртка [llama-cpp-2](https://docs.rs/llama-cpp-2/latest/llama_cpp_2/) (upstream предупреждает о нестабильном API; версии обёртки и backend закреплены, результаты — [gguf-models.md](gguf-models.md), решение — [ADR-0002](../adr/0002-qwen-gguf.md)). Отдельный [llama-server](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md) подключается через внешний локальный endpoint ([внешние модели](../features/models/external-models.md)): пути, тело запроса и семантика scores должны совпасть с HTTP-контрактом bars, слов «OpenAI-compatible» для rerank недостаточно. KV-cache, speculative decoding и tokens/s генерации прямыми оптимизациями bge cross-encoder не являются.

**Подготовка артефактов.** [Microsoft Olive](https://github.com/microsoft/Olive) и [Intel NNCF](https://github.com/openvinotoolkit/nncf) — инструменты преобразования и квантования, не библиотеки инференса Rust; их workflows используют Python, что при Т-3 и В-15 в bars не включается. Готовые веса искать сначала в карточках [BAAI/bge-m3](https://huggingface.co/BAAI/bge-m3) и [BAAI/bge-reranker-v2-m3](https://huggingface.co/BAAI/bge-reranker-v2-m3), затем проверять конкретный экспорт: fastembed rerank использует [rozgo](https://huggingface.co/rozgo/bge-reranker-v2-m3), а не ONNX от BAAI, и надпись INT8/FP16 не доказывает совместимость с EP. У стороннего экспорта фиксируют автора, лицензию, исходную ревизию, conversion recipe, opset, хеши графа и external data, tokenizer, pooling и normalization.

**Перед добавлением зависимости:** закрепить релиз/commit, проверить актуальность releases, лицензию, advisory и поддержку платформы; собрать минимальный пример, проверить один реальный bge/Qwen граф и полный бенч качества ([search-quality.md](search-quality.md)). Сравнительных чисел быстродействия библиотек для bars нет; точные версии кандидатов выбираются при спайке.

## CUDA в сборке bars

Исследование 28-10-2026 для этапа 8.1: можно ли считать на NVIDIA через CUDA, установленную пользователем, и без неё оставаться на Vulkan (GGUF) и DirectML (ONNX), как решено в OQ-209. Метод — чтение исходников закреплённых крейтов в кэше cargo (llama-cpp-sys-2 0.1.160: `build.rs`, `ggml/src/ggml-backend-reg.cpp`, `ggml/src/ggml-backend-dl.cpp`, `src/llama.cpp`, `ggml/src/ggml-cuda/CMakeLists.txt`; llama-cpp-2 0.1.160; ort-sys 2.0.0-rc.13: `build/download/dist.tsv`, `resolve.rs`, `main.rs`; fastembed 7.1.1) и первичной документации. Машина исследования — Intel Arc без карты NVIDIA и без CUDA Toolkit: с CUDA ничего не собиралось и не запускалось. Поведение ниже выведено из кода; проверка на RTX 3090 — замеры этапа 8.1 и реализация.

### llama.cpp: статическая сборка с CUDA не запустится без CUDA

Сейчас llama.cpp вшит в `bars` статически с фичей `vulkan`. Фича `cuda` при статической сборке на Windows линкует import-библиотеки `cudart`, `cublas`, `cublasLt` и `cuda` (`build.rs`: статических cuBLAS для Windows NVIDIA не поставляет). Исполняемый файл тогда зависит от `cudart64_NN.dll`, `cublas64_NN.dll`, `cublasLt64_NN.dll` и `nvcuda.dll` уже при загрузке: без CUDA загрузчик Windows не запустит процесс вовсе, и до отката на Vulkan дело не дойдёт. Значит, статический путь несовместим с «без CUDA — Vulkan».

Рабочий путь — фича `dynamic-backends` (включает `dynamic-link`, `GGML_BACKEND_DL` и `GGML_CPU_ALL_VARIANTS`). `llama.dll`, `ggml.dll` и `ggml-base.dll` лежат рядом с exe, backend'ы — отдельными модулями: `ggml-cuda.dll`, `ggml-vulkan.dll`, варианты `ggml-cpu-*.dll`. До `LlamaBackend::init` вызывается `llama_cpp_2::load_backends_from_path(dir)`, то есть `ggml_backend_load_all_from_path`:

- модули перебираются по списку, CUDA раньше Vulkan, CPU последним;
- каждый модуль грузится `LoadLibraryW` с `SEM_FAILCRITICALERRORS`, поэтому системного окна об отсутствующей DLL нет. Если зависимости `ggml-cuda.dll` (cudart, cuBLAS, nvcuda) не нашлись, модуль пропускается, а Vulkan и CPU грузятся дальше. Модуль, у которого `ggml_backend_score()` вернул 0, тоже отбрасывается;
- `LoadLibraryW` с полным путём ищет зависимые DLL обычным порядком Windows: каталог exe, System32, каталог Windows, текущий каталог, `PATH`. Поэтому DLL CUDA берутся из `PATH`. Если bars сам нашёл каталог CUDA, он может добавить его в `PATH` своего процесса до загрузки модулей.

Побочный выигрыш `GGML_CPU_ALL_VARIANTS` — CPU-вариант llama.cpp выбирается по процессору при запуске. Цена — поставка набора DLL рядом с exe вместо одного статического файла: меняются установщик, `scripts/check.ps1` и [стек](../architecture/stack.md).

**Одна карта в двух backend'ах.** CUDA и Vulkan видят одну и ту же RTX 3090. При сборке списка GPU llama.cpp пропускает устройство с уже встреченным `device_id` (оба backend'а записывают туда PCI bus id) и пишет в лог «skipping device … already using device … with the same id». CUDA регистрируется первой, поэтому модель встаёт на `CUDA0`, а `Vulkan0` той же карты пропускается. Встроенная GPU (тип `IGPU`) добавляется, только если дискретных GPU нет. Вторая дискретная карта другого вендора попадёт в список, и `LlamaModelParams::default()` разложит слои по всем GPU. Нынешний `GgufEmbedder::load` устройств не задаёт. В llama-cpp-2 есть `with_devices` и `with_main_gpu`, поэтому устройство можно выбирать явно. Фактический backend виден по имени устройства (`CUDA0`, `Vulkan0`) через `list_llama_ggml_backend_devices`; `supports_gpu_offload()` говорит только, что GPU есть.

**Сборка.** На машине сборки нужен CUDA Toolkit с `nvcc`; GPU не нужен, если архитектуры заданы явно. Без `native` `ggml-cuda/CMakeLists.txt` для CUDA 13 берёт `75-virtual 80-virtual 86-real 89-real 90-virtual 120a-real 121a-real`; 50/61/70 добавляются только для CUDA младше 13. Готовый архив llama.cpp b11953 `bin-win-cuda-13.4-x64` весит 157 МиБ, отдельный архив его CUDA runtime — 404 МиБ (bars его не поставляет: CUDA ставит пользователь). Размер `ggml-cuda.dll` в сборке bars и время сборки не измерены.

### ONNX Runtime: CUDA EP при статическом ORT

ort-sys 2.0.0-rc.13 автоматически качает готовый ONNX Runtime 1.28.0 от pyke. Для `x86_64-pc-windows-msvc` в `dist.tsv` есть наборы `directml` (архив 31 МБ, используется сейчас), `webgpu`, `nvrtx,directml` и `cuda13,tensorrt,nvrtx,directml` (архив 62 МБ). Фичи `cuda` + `directml` выбирают последний как надмножество. CUDA 12 нет именно в этой таблице автоматического скачивания: `ORT_CUDA_VERSION=12` запросит отсутствующий `cuda12` и завершит сборку ошибкой. Это не ограничение всех поставок ORT или C API закреплённого крейта.

01-11-2026 найден и проверен по SHA-256 официальный динамический GPU-пакет ORT 1.28.0 CUDA12; он опубликован вместе с CUDA13 в [релизе ORT 1.28](https://github.com/microsoft/onnxruntime/releases/tag/v1.28.0). Следовательно, «CUDA12 требует обязательно пересобирать ORT или менять версию ort» неверно. Выбор готового динамического runtime вместо автопакета pyke потребует отдельной проверки линковки Rust, зависимостей и установщика. Его инференс не выполнен: аппаратный preflight отказал по commit. Парный аппаратный опыт CUDA12/DirectML выполнен на одинаковом ORT 1.24.4, не на статическом продукте.

В автопакете pyke ядро ORT остаётся статическим (`onnxruntime.lib`). CUDA EP имеет отдельный интерфейс и [собирается динамическими библиотеками](https://ort.pyke.io/perf/execution-providers). 01-11-2026 в локальном кэше этого архива подтверждены provider DLL; наличие файлов не доказывает их инициализацию. Эти DLL должны поставляться рядом с exe; фича ort `copy-dylibs` копирует их при сборке, а установщику нужно забрать их из target. DirectML в наборе остаётся; проектирование должно явно реализовать попытку CUDA и откат, а не считать его уже готовым.

Предпосылки варианта с автопакетом pyke из исследования 8.1.1; они ещё не приняты как требования продукта:

- по ort — CUDA ≥ 13.2 и cuDNN ≥ 9.23, обе в `PATH`;
- по ORT — major CUDA зависит от выбранного пакета; сборки под cuDNN 8 и 9 не взаимозаменяемы ([ORT CUDA EP](https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html));
- по карте — CUDA 13 убрала Maxwell, Pascal и Volta, минимум — Turing, compute capability 7.5. Карты до Turing CUDA EP не получат и остаются на Vulkan и DirectML.

**Откат и причина.** Если регистрация EP не удалась, ort по умолчанию молча продолжает на CPU. Список `[CUDA, DirectML]` без `error_on_failure` у CUDA перейдёт на DirectML незаметно, и причины не будет. Сейчас bars регистрирует DirectML с `error_on_failure` и сам держит причину отката GPU→CPU. Для CUDA тот же приём: регистрация с `error_on_failure`, при ошибке — новая сессия с DirectML и сохранённая причина. `ExecutionProvider::is_available()` в ort спрашивает `GetAvailableProviders`, то есть есть ли EP в сборке ORT; установленную CUDA он не подтверждает. fastembed 7.1.1 принимает `with_execution_providers` для embed и rerank, поэтому менять fastembed не нужно. FP16 embed сейчас требует DirectML (`builtin.rs`). Работает ли он на CUDA, проверяется замерами.

**Совместная загрузка CUDA-моделей.** `ggml-cuda.dll` и CUDA EP требуют cudart той major-версии, под которую собраны. Разные major имеют разные имена DLL и требуют соответствующих зависимостей. При выборе автопакета pyke CUDA13 логично исследовать llama.cpp той же major, чтобы не требовать две поставки. Это вариант проектирования, не сделанная сборка. Совместная инициализация обеих моделей, память arena и GPU-лимиты в одном продуктовом процессе ещё не проверены.

### Обнаружение установленных CUDA и cuDNN

| Что | Где искать | Версия и условие |
|---|---|---|
| Драйвер NVIDIA | `nvcuda.dll` в System32, ставится драйвером; без DLL CUDA Driver API недоступен | `cuDriverGetVersion` сообщает уровень Driver API, например 13020 = 13.2, но не установленный toolkit; простого условия «версия драйвера ≥ версия runtime» недостаточно |
| Карта | `cuInit`, `cuDeviceGetCount`, `cuDeviceGetAttribute` из `nvcuda.dll` | compute capability ≥ 7.5 для CUDA 13; PCI bus id для сопоставления с Vulkan/DXGI |
| CUDA runtime и cuBLAS | `CUDA_PATH` (ставит установщик toolkit, по умолчанию `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.x`), затем `PATH`; файлы `cudart64_13.dll`, `cublas64_13.dll`, `cublasLt64_13.dll` в `bin`. Есть ли у CUDA 13 подкаталог `bin\x64`, первичным источником не подтверждено (такая раскладка есть у pip-колёс CUDA 13), поэтому проверять оба | `cudaRuntimeGetVersion` из загруженной cudart. С CUDA 13.1 драйвер в toolkit не входит |
| cuDNN | `PATH`, затем `C:\Program Files\NVIDIA\CUDNN\v9.*\bin` с подкаталогами. Для zip-архива NVIDIA велит добавить `bin` в `PATH` вручную; раскладку графического установщика (несколько вариантов под разные CUDA в одном пакете) [документация](https://docs.nvidia.com/cudnn/installation/backend/windows) не описывает. Файлы `cudnn64_9.dll` и соседние `cudnn_*64_9.dll` | `cudnnGetVersion` (92300 = 9.23.0) и `cudnnGetCudartVersion` — под какую CUDA major собран |

Отсутствие `nvidia-smi` в `PATH` ничего не доказывает. Проверку стоит делать через `LoadLibraryExW` с подавлением системных окон и чтение экспортов — это те же DLL, которые затем возьмут `ggml-cuda.dll` и CUDA EP. Итог проверки — конкретная причина для окна: «драйвер NVIDIA не найден», «драйвер поддерживает CUDA 12.4, нужна 13.2», «CUDA Toolkit не найден», «cuDNN не найден» или «карта 6.1 не поддерживается CUDA 13». Лог ggml о пропущенном модуле такой причины не даёт.

По [minor-version compatibility NVIDIA](https://docs.nvidia.com/deploy/cuda-compatibility/minor-version-compatibility.html), сверенной 02-11-2026, внутри CUDA major возможна работа с ограничениями на более раннем драйвере. Нужны совместимость конкретных компонентов, `cuInit` и настоящий запуск kernels; поддержка нового PTX/функций может требовать обновления. Нельзя отвергать runtime 13.2 только по значению Driver API 13.0. При несовместимой major или реальном отказе возвращается точная причина драйвера, а не предположение по `nvidia-smi`.

В закреплённом `ort 2.0.0-rc.13` helper `ep::cuda::preload_dylibs` и константа `CUDA_DYLIBS` перечисляют CUDA12, хотя автопакет использует CUDA13. Это подтверждено исходником `src/ep/cuda.rs`, повторно прочитанным 02-11-2026; [закреплённый исходник](https://github.com/pykeio/ort/blob/v2.0.0-rc.13/src/ep/cuda.rs). Его нельзя считать готовым обнаружением CUDA13: требуется загрузка зависимостей выбранного артефакта с проверкой фактических версий.

### Откат на Vulkan и DirectML

| Модели | Путь CUDA | Когда откат | Куда |
|---|---|---|---|
| GGUF (llama.cpp) | `ggml-cuda.dll` загрузился и видит устройство | модуль не загрузился, CUDA-устройств нет или ошибка загрузки модели на CUDA | Vulkan той же карты, затем CPU |
| ONNX (fastembed/ORT) | CUDA EP зарегистрирован с `error_on_failure` | регистрация или первая сессия упали | DirectML, затем CPU |

Откат во время работы уже есть у GGUF (GPU→CPU в `GgufEmbedder::one`). Для CUDA→Vulkan нужна повторная загрузка модели с явно выбранным Vulkan-устройством: выгруженный модуль CUDA останется в списке устройств процесса.

### Выводы для проектирования CUDA

1. llama.cpp — только через `dynamic-backends`; статический `cuda` ломает запуск без CUDA.
2. ONNX — автопакета pyke CUDA13 недостаточно для выбранной автором 02-11-2026 поддержки CUDA12 и CUDA13. Готовый динамический ORT CUDA12 существует; его Rust-интеграция и наличие DirectML в согласованном комплекте отдельно проверяются. Версия crate сама по себе не запрещает динамическую поставку.
3. Автор выбрал обе major, не обязательную установку CUDA13. Одна major на процесс — согласованная часть проекта из-за глобального ORT/ggml и одноимённых cuDNN DLL; она не ограничивает продукт одной поддерживаемой major.
4. bars должен различать установленный драйвер, библиотеки CUDA/cuDNN и успешный инференс, сохранять фактические пути и причины отказа; изменение системного PATH из этого не следует.
5. Устройство GGUF выбирается явно, а не «все GPU».

OQ-224 закрыт 02-11-2026 ответом автора «Нужна поддержка cuda 12 и 13»; затем автор принял переработанный проект и дополнение Т-3 (OQ-225/241). Согласованный контракт, ещё не реализованный в продукте, — [встроенные модели](../features/models/local-models.md), разделы «Проект CUDA».

### Две major CUDA: проверенные API и незакрытый риск поставки

Сверка 02-11-2026: fastembed 7.1.1 имеет `ort-load-dynamic = ["ort/load-dynamic"]`, тогда как `fastembed/cuda` включает candle. В `ort rc.13` доступны абсолютный `init_from` и глобальные `G_ORT_LIB`/`G_ORT_API` (`OnceLock`, исходники `src/lib.rs`, `src/environment.rs`). Это основание выбрать библиотеку до первого вызова и не обещать горячую подмену ORT. Сверены локальные закреплённые исходники; такой выбор в bars пока не реализован.

[Исходный CMake ORT 1.28](https://github.com/microsoft/onnxruntime/blob/v1.28.0/cmake/CMakeLists.txt) содержит независимые `onnxruntime_USE_CUDA`, `onnxruntime_USE_DML` и сборку shared library; [инструкция Microsoft](https://onnxruntime.ai/docs/build/eps.html) описывает необходимые SDK. Это основание исследовать согласованные собственные CUDA+DirectML-комплекты, но не доказательство прошедшей комбинированной сборки. Наличие CUDA в готовом GPU-архиве не подтверждает DirectML. Смешивание provider DLL произвольных архивов не принимается как решение ABI.

[Таблица CUDA EP Microsoft](https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html), сверенная 02-11-2026, указывает для ORT 1.28 CUDA12.8+ либо CUDA13.0+ и cuDNN9. Это требования официальных GPU-сборок, отличные от CUDA13.2/cuDNN9.23 автопакета pyke. Точные минимумы собственных сборок, архитектуры и Windows-зависимости подтверждаются их manifest и отдельными прогонами; существующие аппаратные данные 8.1.2 не принимают ни одну новую поставку.

## Что не доказано для bars

На RTX 3090 проверены готовые GGUF CUDA/Vulkan и ORT 1.24.4 CUDA/DirectML: численные отклонения, типы KV, I/O Binding, фиксированный CUDA Graph и CUDA-размещение вычислительных узлов bge. Результаты и ограничения принадлежат [исследованию ускорителей](inference-speed.md), раздел «RTX 3090: отбор ускорителей 8.1.2»; чистый выигрыш скорости не доказан, TensorRT остановлен защитой commit.

Не проверены fastembed-адаптеры, полный retrieval изменённых embeddings, OpenVINO, совместная загрузка двух CUDA-моделей в bars, запуск dynamic-backends без установленной CUDA и дедупликация CUDA0/Vulkan0 в одном процессе. Продуктовая CUDA не собрана и не запущена. Наличие provider DLL в кэше не является аппаратной приёмкой ORT 1.28; минимальные версии зависимостей и поставка ещё требуют проектирования. Таблица устройств остаётся инженерными ожиданиями.
