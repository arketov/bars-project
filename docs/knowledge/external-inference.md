---
title: Другие серверы embedding и rerank — отбор для bars
---

# Другие серверы embedding и rerank — отбор для bars

Статус 23-10-2026: выбраны и реализованы llama.cpp через совместимый HTTP и отдельный источник TEI. llama.cpp b11669/Vulkan с Qwen3 embed/rerank и TEI 1.9.4/CUDA с multilingual MiniLM embed/rerank прошли настоящий индекс и поиск bars на RTX 3090. Infinity/vLLM изучены по первичным API, аппаратно не проверены и не входят в выбранные варианты. Сравнительное превосходство моделей не установлено. Прежнее исследование [Qwen GGUF](gguf-models.md) проверяет встроенный backend, а не HTTP-сервер.

## Задача и критерии отбора серверов

Пожелание автора: более качественные embedding и rerank на домашней RTX 3090 через отдельные инструменты. Критерии: отдельные модели обоих этапов, настоящий численный rerank, документированный API, Windows либо отдельный Linux-сервер, отсутствие зависимости серверного runtime внутри bars. Ускорение или улучшение качества требуют одинакового корпуса и запросов; наличие API этого не доказывает.

## Кандидаты серверов

| Сервер | API и модели | Решение для bars |
|---|---|---|
| llama.cpp | `/v1/embeddings`; `/rerank` с `documents`, объект `results`. GGUF Qwen3-Embedding, BGE reranker; поддержка конкретного Qwen reranker зависит от версии | Основной путь Windows: `local-openai`, разные порты для embed/rerank; существующий транспорт, без установки runtime в bars |
| Infinity | `/embeddings` (версионный префикс настраивается), `/rerank`, OpenAI/Cohere-подобные тела; sentence-transformers/torch/ONNX | Совместимый HTTP через `local-openai`; адрес должен соответствовать префиксу `/v1`, который использует bars. Отдельный Python-сервис пользователя, Python в продукт не добавляется |
| Hugging Face TEI | Нативные `/embed` с `inputs`, `/rerank` с `texts`; оба ответа — массивы. BGE-M3 и sequence-classification reranker, в том числе multilingual GTE; RTX 3090 относится к Ampere 86 | Новый источник `tei`; самостоятельный Linux/контейнерный сервер, два экземпляра для двух моделей |
| vLLM | OpenAI embedding, Cohere rerank; pooling/score поддерживаются для определённых архитектур | Совместимый HTTP возможен; отдельный продуктовый источник не нужен. Native Windows отсутствует; WSL/Linux. Не выбран для первого локального приёмочного прогона |

Источники на 23-10-2026: [llama.cpp server](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md), [Infinity](https://github.com/michaelfeil/infinity), [TEI API и типы](https://github.com/huggingface/text-embeddings-inference/blob/main/router/src/http/types.rs), [TEI модели и GPU](https://huggingface.co/docs/text-embeddings-inference/supported_models), [vLLM pooling](https://docs.vllm.ai/en/stable/models/pooling_models/), [платформы vLLM](https://docs.vllm.ai/en/stable/getting_started/installation/gpu/).

## Выявленная несовместимость TEI

Нынешний `HttpReranker` отправляет `documents` и разбирает `results`/`data`. TEI требует `texts` и возвращает голый массив `{index, score}`. Без адаптера это ошибка запроса/разбора, а не отсутствие модели. Нативный embed TEI позволяет явно запретить усечение через `truncate: false`; OpenAI-совместимый маршрут такой гарантии не даёт. Поэтому выбран отдельный протокол, а не перебор форматов при ошибке.

Контракт реализации — [внешние модели](../features/models/external-models.md). Два сервера TEI на разных портах обслуживают независимые модели; имя в настройках фиксирует идентичность для Т-25, но нативный API TEI модель не переключает.

## Ограниченная проверка llama-server на RTX 3090

23-10-2026: portable Vulkan b11669, commit `620d2225c`; архив SHA-256 `2c07574e7bde71321cb785282b20cc821e951132149eeceeea32ddebdbf0598e`. Готовые Qwen3-Embedding-0.6B и Qwen3-Reranker-0.6B Q8_0 прочитаны только с D:. Два сервера на loopback 18083/18084; `-ngl 99`, контекст/пачка/микропачка 4096, один слот; embed с `--embedding --pooling last`, rerank с `--reranking`. Список Vulkan-устройств — RTX 3090; общий VRAM во время пробы 11431 МиБ, это не атрибуция памяти отдельной модели.

Прямой embed дал 2×1024 координаты; rerank для вопроса об овощах поставил борщ выше документа о поиске: 0,99986 против 0,000014. Настоящий `ConfiguredProvider`/`WorkerHandler`, отдельные профиль/индекс: три документа, сохранение и чтение настроек, два русскоязычных запроса с ожидаемым лидером и `score_kind=rerank`, без `rerank_error`; 246/87 мс на уже прогретых серверах. Стенд `crates/bars-worker/examples/verify_external_inference.rs`; исходные ответы и хеши — в материалах сессии external-inference от 23-10-2026. Оба собственных сервера остановлены, веса сохранены; рабочий bars не использовался.

Два вопроса малого корпуса проверяют подключение, а не качество поиска. Для сравнения качества нужны прежний замороженный корпус и вопросы, одинаковые настройки/длины, baseline BGE, холодные и тёплые задержки, VRAM/RAM; результаты встроенного Q4 не переносятся на внешний Q8. Такой сравнительный эксперимент не проводился.

## Проверка TEI на RTX 3090 через WSL2

23-10-2026: TEI 1.9.4, revision `e80ef225ed0e6cb1717ce632a6a84b6cf211bb67`, официальный CUDA-бинарник из OCI-образа, существующая Ubuntu 24.04.2/WSL2. Индекс образа `sha256:bee0bdb887096229add91b6fdfcf95258c12eb9180e90dc86f3b4b25b86fc613`; SHA-256 бинарника `50360a0313d93ae0215f186b73c0590192dfebc859fef8b4e4ea671db66c974a`. Два сервера на loopback 18085/18086, float16, `max-batch-tokens=2048`, `max-concurrent-requests=8`, два tokenization workers. Логи обоих подтверждают `Starting FlashBert model on Cuda`; GPU — RTX 3090.

Модели — `sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2` (revision `e8f8c211226b894fcb81acc59f3b34ba3efd5f42`) и `cross-encoder/mmarco-mMiniLMv2-L12-H384-v1` (revision `1427fd652930e4ba29e8149678df786c240d8825`). Веса и конфигурации сохранены только на D:, версии и хеши зафиксированы. Это диагностические модели для совместимости, выбор лучшей модели ими не обосновывается.

Настоящие `ConfiguredProvider`/`WorkerHandler` в изолированном профиле: индекс трёх документов, сохранение/чтение настроек, два русскоязычных поиска с правильным лидером, все три результата после rerank, без `rerank_error`; 309/249 мс на прогретых серверах. Отдельно настоящие `HttpEmbedder` и `HttpReranker` получили ожидаемый 422 Validation на входе 18432 символа. TEI `/info` показывает `auto_truncate:true`, но запрос bars с `truncate:false` запрещает усечение: сервер отклонил 2050 токенов при пределе 128 для embed и 2056 при пределе 512 для rerank. Доказательства в материалах сессии external-inference от 23-10-2026: `tei-verified.json`, `tei-info.json`, `tei-backends.json`, журналы серверов и хеши моделей. Собственные процессы остановлены SIGINT с проверкой исполняемого файла; веса сохранены, рабочий bars и его профиль не менялись.

Docker Desktop 4.61.0 не потребовался: его первоначальный запуск завершился ошибкой Inference Manager/сокета `dockerInference`, без ремонта или сброса. Загрузка официального бинарного слоя через HTTP/1.1 с продолжением Range после обрыва завершилась полной проверкой SHA-256. Linux runtime сохранён для воспроизведения в `/root/Projects/oss/bars-tei-5-3`.
