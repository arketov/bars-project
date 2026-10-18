---
title: Библиотеки инференса для bars
---

# Библиотеки инференса для bars

Обзор первичных источников на 18-10-2026. Это каталог кандидатов, не решение расширить стек. Действующий контракт — Т-3, Т-17; закреплённые зависимости workspace: fastembed `=7.1.1`, ort `=2.0.0-rc.13`, candle-core `0.11`. Текущие upstream-функции не считаются доступными этой сборке автоматически. Аппаратных сравнений и новых зависимостей в этой работе нет.

## Близкие к текущему ONNX-пути варианты

| Библиотека / runtime | Применение | Цена интеграции и приоритет исследования |
|---|---|---|
| [fastembed-rs](https://github.com/Anush008/fastembed-rs) | Каталог, tokenizer, embed/rerank, ONNX и отдельные candle-модели | Сохранить как базовый путь; проверить доступность нужных SessionOptions до проектирования настроек |
| [ort](https://docs.rs/ort/2.0.0-rc.13/ort/) + ONNX Runtime | Прямое управление сессиями, EP и тензорами из Rust | Следующий кандидат, если fastembed не открывает нужную настройку; вручную сохранить pooling, normalization, tokenizer и порядок rerank |
| [OpenVINO EP](https://onnxruntime.ai/docs/execution-providers/OpenVINO-ExecutionProvider.html) | Intel CPU/GPU/NPU через ORT | Высокий приоритет спайка на Intel; проверить ABI, упаковку и поддерживаемые графы |
| [OpenVINO Runtime](https://docs.openvino.ai/2026/index.html) напрямую | Независимое управление compile_model, устройствами и запросами | C/C++ runtime и Rust FFI/обёртка; больше собственного кода и отдельный жизненный цикл |
| [Windows ML](https://learn.microsoft.com/en-us/windows/ai/new-windows-ml/overview) | Поставка и выбор аппаратных EP на Windows | Перспективный путь Windows; отдельно проверить Rust API, доступность EP офлайн, ОС и совместимость с нынешним ORT |
| [CUDA EP](https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html) | NVIDIA GPU | Проверять на NVIDIA; CUDA/cuDNN, дистрибутив и версии зависимостей |
| [TensorRT EP](https://onnxruntime.ai/docs/execution-providers/TensorRT-ExecutionProvider.html) | NVIDIA, компиляция engine и профили форм | После CUDA baseline; cold start, cache, VRAM и поставка runtime |
| [CoreML EP](https://onnxruntime.ai/docs/execution-providers/CoreML-ExecutionProvider.html) | Mac: CPU/GPU/ANE | Проверить на Mac; разрешение ANE не гарантирует выполнение всех узлов на нём |
| [QNN EP](https://onnxruntime.ai/docs/execution-providers/QNN-ExecutionProvider.html) | Qualcomm NPU | Только для соответствующего оборудования; статические формы, SDK и precision |
| [MIGraphX EP](https://onnxruntime.ai/docs/execution-providers/MIGraphX-ExecutionProvider.html) | AMD GPU | Проверить поддерживаемые ОС/GPU; не считать заменой DirectML на любом Windows PC |

Особенность актуального OpenVINO: ORT помечает прежний встроенный EP deprecated и рекомендует [отдельный Plugin EP](https://github.com/onnxruntime/onnxruntime-ep-openvino/blob/main/docs/QuickStart.md). Поэтому старый путь «обязательно пересобрать ORT с OpenVINO» больше не единственный. Это не доказательство, что Plugin EP можно загрузить в статический runtime bars: сначала проверить версию C API, регистрацию плагина, архитектуру и native libraries. Не смешивать DLL разных ABI в одном процессе.

## GGUF и llama.cpp

[llama.cpp](https://github.com/ggml-org/llama.cpp) — C/C++ runtime с CPU и несколькими GPU backend, включая Vulkan, CUDA и Metal; GGUF и кванты позволяют снижать память. Наличие backend не гарантирует поддержку архитектуры, pooling и конкретного кванта модели. Для bars это кандидат прежде всего для крупных Qwen3-Embedding, а не автоматическая замена ONNX bge-m3.

Пути подключения:

- Встроенный [llama-cpp-2](https://docs.rs/llama-cpp-2/latest/llama_cpp_2/) — Rust-обёртка; upstream прямо предупреждает о нестабильном API вслед за llama.cpp. Зафиксировать версии обёртки и C++ backend, проверить сборку вместе с ORT, управление памятью и потоками.
- Отдельный [llama-server](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md) — сначала проверить через уже предусмотренный внешний локальный endpoint. Сервер поддерживает embeddings и reranking для соответствующих моделей, но paths, тело запроса и семантика scores должны совпасть с HTTP-контрактом bars. Слова «OpenAI-compatible» недостаточно для rerank.

Сравнивать FP16/Q8/Q4 на одинаковом корпусе; экономия RAM может быть важнее скорости. KV-cache, speculative decoding и tokens/s генерации не являются прямыми оптимизациями bge cross-encoder: это другая нагрузка. До смены встроенного backend — спайк и ADR, без изменения Т-3 по инициативе агента.

## Rust-альтернативы и внешний сервер

| Проект | Чем полезен | Ограничение для bars |
|---|---|---|
| [Candle](https://github.com/huggingface/candle) | Rust tensor/model framework, CPU/CUDA/Metal; уже используется частью моделей | GPU Candle не означает DirectML/NPU. Текущий bars выбрал CPU f32; перенос модели требует проверки kernels, precision и загрузчика |
| [Burn](https://github.com/tracel-ai/burn) | Rust framework с WGPU/CUDA и fusion; кандидат для переносимых GPU-вычислений | Не замена fastembed одной строкой: импорт/операторы, модель и tokenizer/postprocessing требуют проверки |
| [tract](https://github.com/sonos/tract) | Самодостаточный Rust inference runtime для ONNX, в первую очередь CPU-путь | Кандидат для облегчённой поставки; совместимость bge-графов, shapes и производительность не проверены |
| [Text Embeddings Inference, TEI](https://github.com/huggingface/text-embeddings-inference) | Rust-сервер embed/rerank: token-based dynamic batching, оптимизированные transformer kernels | Кандидат внешнего локального сервера и ориентир для batching; сборка/поддерживаемая платформа и HTTP-контракт проверяются отдельно |

Ни один из этих вариантов не принят как новый backend bars. TEI полезен при устойчивом потоке запросов, но ожидание заполнения пачки может ухудшить одиночную latency. Установка сервера и владение его процессом — отдельная продуктовая работа, не часть обзора.

## Инструменты подготовки моделей

[Microsoft Olive](https://github.com/microsoft/Olive) автоматизирует преобразования, квантование и поиск конфигурации под оборудование. [Intel NNCF](https://github.com/openvinotoolkit/nncf) — компрессия/квантование для OpenVINO. Это инструменты подготовки артефактов, не библиотеки инференса Rust. Их основные workflows используют Python; при текущем Т-3/В-15 не включать их в bars или обязательную сборку. Использование отдельного pipeline подготовки потребует явного решения о допустимых инструментах; здесь он не запускался.

Готовые веса искать сначала в исходных карточках [BAAI/bge-m3](https://huggingface.co/BAAI/bge-m3) и [BAAI/bge-reranker-v2-m3](https://huggingface.co/BAAI/bge-reranker-v2-m3), затем проверять конкретный экспорт. Текущий fastembed reranker использует [rozgo](https://huggingface.co/rozgo/bge-reranker-v2-m3), а не ONNX от BAAI. Наличие ONNX или надписи INT8/FP16 не доказывает совместимость с EP.

Для стороннего экспорта фиксировать автора, лицензию, исходную ревизию, conversion recipe, opset, хеши графа и external data, токенизатор, pooling и normalization. В этом исследовании нет проверенного на bars нового FP16/INT8-артефакта: каталог вариантов не выдаётся за готовый заменяемый файл. Уменьшенный [mmarco-mMiniLMv2](https://huggingface.co/cross-encoder/mmarco-mMiniLMv2-L12-H384-v1) — отдельный кандидат модели; он меняет качество и калибровку оценок, а не только runtime.

## Как выбрать кандидата для следующего спайка

Предложение: сначала настройки нынешнего fastembed/ORT; затем OpenVINO GPU на Intel и FP16 DirectML; отдельно CPU INT8. Для больших candle-моделей — llama.cpp/GGUF; для NPU — сначала успешная компиляция статического графа, потом latency и энергия. Burn/tract — резерв при конкретной причине менять runtime. CUDA/TensorRT и CoreML требуют соответствующей машины.

Перед добавлением зависимости: закрепить релиз/commit, проверить актуальность releases и исправлений, лицензию, advisory и поддержку платформы; собрать минимальный пример, проверить один реальный bge/Qwen граф и полный quality benchmark. В ходе этого обзора зависимости не добавлялись: точные версии кандидатов выбираются при спайке, а не объявляются «последними» по плавающему README.

Все внешние ссылки проверены 18-10-2026. Сравнительных чисел быстродействия библиотек для bars пока нет; порядок спайков — предложение исследования.
