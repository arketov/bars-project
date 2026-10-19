---
title: Устройства, DirectML и библиотеки инференса — обзор вариантов
---

# Устройства, DirectML и библиотеки инференса

[← База знаний](Index.md). Обзор первичных источников на 18-10-2026. Это каталог кандидатов и ограничений, а не решение расширить стек: действующий контракт — Т-3, Т-17; закреплённые зависимости workspace — fastembed `=7.1.1`, ort `=2.0.0-rc.13`, candle-core `0.11`, llama-cpp-2 `0.1.160`. Текущие upstream-функции не считаются доступными этой сборке автоматически. Измерения i7-11700KF / RTX 3090 и выбранный FP16 rerank — [inference-speed.md](inference-speed.md); других аппаратных платформ в замерах нет, рекомендации для них — гипотезы. Какие устройства и precision используются в продукте — [встроенные модели](../features/models/local-models.md).

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

**Диагностика неверных оценок (ISS-03).** Сначала идентифицировать модель: ревизия, граф и все external data с хешами, tokenizer, max_length, имена и типы входов/выходов. Повторить одинаковые пары на CPU и DML, записать сырые логиты до sigmoid и порядок после неё; проверить несколько длин, кириллицу, NaN/Inf. Почти нулевая sigmoid может скрывать крупную ошибку отрицательных логитов. Если артефакт корректен на CPU и ошибочен на DML, область сужается до EP/драйвера/precision, но виновник не определён; переключение на CPU — обход, а не доказательство причины. Существующая диагностика — [local-models.md](../features/models/local-models.md); данных с проблемной машины нет.

## Другие GPU и EP

- **NVIDIA.** Первый кандидат — [CUDA EP](https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html): зависит от совместимых CUDA/cuDNN; CUDA Graphs требуют соблюдения ограничений по формам и адресам. [TensorRT EP](https://onnxruntime.ai/docs/execution-providers/TensorRT-ExecutionProvider.html) — после CUDA baseline: компиляция engine, profiles min/opt/max для динамических форм, cache, VRAM и поставка runtime; выигрыш считают с учётом холодного старта.
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

## Что не доказано для bars

Нет измеренного выигрыша buckets, binding или OpenVINO над текущим DirectML; не установлено размещение всех операторов bge-графов; не проверены изменения API сессии через fastembed 7.1.1. Критерии результата — latency, память, качество и фактическое устройство по [протоколу замеров](inference-speed.md#протокол-воспроизведения-замеров). Таблица устройств — инженерные ожидания, а не результаты сравнительного теста bars.
