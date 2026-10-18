---
title: Инференс на CPU, GPU и NPU
---

# Инференс на CPU, GPU и NPU

Обзор на 18-10-2026, первичные источники проверены. Измерения i7-11700KF/RTX 3090 и выбранный FP16 rerank — [inference-speed.md](inference-speed.md). Другие аппаратные платформы в этой сессии не измерены; рекомендации для них — гипотезы, не выбор нового стека. Варианты библиотек — [inference-libraries.md](inference-libraries.md).

## Выбор устройства зависит от нагрузки

| Устройство | Когда имеет смысл проверить | Основные издержки |
|---|---|---|
| CPU | Одиночный короткий запрос, универсальный baseline | Пропускная способность памяти, конкуренция пулов, ISA и число потоков |
| Встроенная GPU | Индексация и пачки rerank без отдельной VRAM | Общие с CPU память и тепловой бюджет; очереди, синхронизация, размещение буферов |
| Дискретная GPU | Большие матричные вычисления и устойчивый поток пачек | Передача данных, VRAM весов и активаций, запуск kernels, зависимости runtime |
| NPU | Поддерживаемый граф для длительной фоновой работы с целью снизить энергопотребление | Компиляция, ограничения форм/операторов/precision, зависимость от поколения и драйвера |

CPU не обязан проигрывать GPU на коротком запросе, NPU не обязан выигрывать по задержке. TOPS разных устройств без одинаковых precision, sparsity и модели не позволяют предсказать время bars. Windows ML отдельно описывает NPU как путь для энергоэффективной длительной работы, GPU — для throughput, CPU — как универсальный вариант. [Microsoft: Windows ML](https://learn.microsoft.com/en-us/windows/ai/new-windows-ml/overview).

Вывод для bars: embed_query, фоновую индексацию и rerank нужно оценивать раздельно. Размещение embed и rerank на разных устройствах — возможный опыт, но действующая настройка `[device]` общая; исследование не добавляет новых переключателей.

## CPU: инструкции, память, потоки

Поддержка ISA у бинарника и наличие быстрых kernels для precision — разные проверки. AVX2/VNNI/BF16 не следует выводить из одного слова «Intel» или «AMD». FP32 — контроль; INT8 — кандидат; FP16 без нужного CPU kernel может вообще не исполниться. Матрица потоков и конкуренция с индексацией описаны в [inference-speed.md](inference-speed.md).

Текущие candle-модели bars исполняются на CPU в f32; наличие GPU в системе не меняет этот путь. Большие веса и активации способны вытеснять память в paging; ускорение matmul не исправит постоянные page faults. Снижать нужно рабочий набор, число одновременно живущих моделей или precision, а не автоматически увеличивать файл подкачки.

## GPU: покрытие графа важнее флага «GPU включён»

Execution provider принимает поддерживаемые части ONNX-графа; остальные могут остаться CPU. Границы между частями означают дополнительные синхронизации и обмены. Регистрация EP доказывает доступность провайдера, но не исполнение всей модели на нём. Сопоставлять нужно профиль узлов, latency и наблюдение устройства. [ORT execution providers](https://onnxruntime.ai/docs/execution-providers/).

Встроенная GPU использует общую физическую память, но это не означает бесплатные копирования ORT и отсутствие CPU/GPU synchronization. На ноутбуке рост CPU-потоков способен уменьшить доступный GPU тепловой бюджет. Поэтому общую загрузку и режим питания фиксируют для каждой серии.

На NVIDIA первый кандидат — CUDA EP; его сборка зависит от совместимых CUDA/cuDNN. CUDA Graphs требуют соблюдения ограничений, включая формы и адреса, и не заменяют обычный baseline. [ORT CUDA](https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html). TensorRT — отдельный кандидат: компиляция engine, profiles min/opt/max для динамических форм, engine/timing cache и дополнительные зависимости. Выигрыш сравнивают с CUDA после включения стоимости холодного старта. [ORT TensorRT](https://onnxruntime.ai/docs/execution-providers/TensorRT-ExecutionProvider.html).

На AMD Windows для широкого набора GPU остаётся кандидат DirectML; MIGraphX требует проверки собственной платформенной матрицы, его нельзя считать универсальным Windows-бэкендом. [ORT MIGraphX](https://onnxruntime.ai/docs/execution-providers/MIGraphX-ExecutionProvider.html). На Intel стоит сравнить DirectML и OpenVINO GPU при одинаковых модели, длинах и precision.

## Intel NPU и Core Ultra 5 125H

Машина исторического спайка — Core Ultra 5 125H (Meteor Lake) с Intel AI Boost. DirectML-адаптер 1 в том опыте дал ошибку; это не проверка исправности NPU. DirectML GPU и Intel NPU — разные пути выполнения.

OpenVINO предоставляет Intel NPU backend, но произвольный ONNX не становится поддержанным автоматически. В документации 2026 динамические формы NPU имеют статус preview: начиная с NPU40XX/Lunar Lake, bounded shapes, определённые драйверы, Compiler-In-Plugin; проверенное покрытие ограничено vision. Это не основание ожидать динамический transformer на Meteor Lake. Начальный кандидат для 125H — статические batch и sequence length, затем проверка компиляции и результатов конкретного графа. Совместимость скомпилированных blobs между версиями OpenVINO не гарантирована. [OpenVINO NPU device](https://docs.openvino.ai/2026/openvino-workflow/running-inference/inference-devices-and-modes/npu-device.html).

Набор buckets увеличивает число сессий/скомпилированных вариантов и расход памяти. Начинать разумно с одного статического графа для коротких запросов и отдельного для rerank; скорость и энергия bge-моделей на NPU bars пока неизвестны. Отказ компиляции — результат проверки совместимости, не повод молча засчитать CPU fallback как NPU.

## Другие NPU: Qualcomm, Apple, AMD

Qualcomm QNN EP использует собственные backend и SDK. Для HTP проверяют квантованный граф и калибровку; динамические формы ONNX нужно фиксировать. Это не путь к Intel NPU. [ORT QNN](https://onnxruntime.ai/docs/execution-providers/QNN-ExecutionProvider.html).

CoreML на Apple позволяет выбирать compute units: CPU, CPU+GPU, CPU+NeuralEngine или ALL. Разрешение ANE не гарантирует размещение каждого узла на ANE; динамические формы способны ухудшать скорость. Проверять compute plan и конкретную версию macOS; macOS в bars аппаратно не проверена. [ORT CoreML](https://onnxruntime.ai/docs/execution-providers/CoreML-ExecutionProvider.html).

Для AMD NPU следует проверять поставляемый для конкретного Ryzen AI execution provider и поддерживаемую модель через [Windows ML EP mapping](https://learn.microsoft.com/en-us/windows/ai/new-windows-ml/overview). Ни DirectML GPU, ни название «ONNX Runtime» сами по себе не подтверждают доступ к AMD NPU; совместимость с текущим Rust/ORT bars не проверена.

## Windows ML и поставка ускорителей

Windows ML — слой поставки и выбора hardware EP поверх ONNX Runtime. CPU и DirectML доступны на поддерживаемых им версиях Windows; оптимизированные EP для NPU и отдельных GPU требуют Windows 11 24H2 или новее. Это ограничение нельзя незаметно распространить на все установки bars. [Требования Windows ML](https://learn.microsoft.com/en-us/windows/ai/new-windows-ml/overview).

При оценке любого ускорителя нужно проверить: ОС/архитектуру, драйвер, ABI runtime и EP, размер дистрибутива, загрузку зависимостей без сети, лицензии, холодный старт и наблюдаемую причину fallback. Показать устройство в интерфейсе мало: для результата нужны профиль исполнения и правильные выходы.

Все внешние ссылки — первичные источники; дата обращения 18-10-2026. Таблица устройств содержит инженерные ожидания, а не результаты сравнительного теста bars.
