---
title: CUDA в сборке bars — итог исследования 8.1.1
---

# CUDA в сборке bars — итог 8.1.1

## Сделано

Исследование 8.1.1 завершено 28-10-2026: раздел «CUDA в сборке bars» в [обзоре устройств инференса](../../knowledge/inference-devices.md). Метод — исходники закреплённых llama-cpp-sys-2/llama-cpp-2 0.1.160, ort-sys 2.0.0-rc.13, fastembed 7.1.1 и первичная документация ort, ONNX Runtime, NVIDIA, llama.cpp.

Главное:
- статический llama.cpp с фичей `cuda` на Windows зависит от `cudart64_NN.dll`, cuBLAS и `nvcuda.dll` при загрузке exe — без CUDA bars не запустится; рабочий путь — `dynamic-backends` (`ggml-cuda.dll`, `ggml-vulkan.dll` грузятся `LoadLibraryW`, отсутствующий модуль пропускается);
- одна карта в CUDA и Vulkan дедуплицируется по PCI bus id, CUDA регистрируется первой;
- ort rc.13 даёт ORT 1.28.0 только под CUDA 13 (`cuda13,tensorrt,nvrtx,directml`, 62 МБ); ядро статическое, CUDA EP — DLL рядом с exe; нужны CUDA ≥ 13.2, cuDNN ≥ 9.23, карта от Turing;
- откат с причиной: регистрация CUDA EP с `error_on_failure`, затем DirectML; для GGUF — Vulkan; собственная проверка драйвера, CUDA и cuDNN по экспортам DLL (таблица в исследовании).

Также изменены: QUESTIONS.md (OQ-224, OQ-225), оглавление `docs/knowledge/Index.md`. Код не менялся.

## Не сделано

Ничего не собрано и не запущено с CUDA: на машине сессии нет NVIDIA. Не проверены загрузка `ggml-cuda.dll` без CUDA, дедупликация `CUDA0`/`Vulkan0`, содержимое архива ORT `cuda13`, раскладка CUDA 13 (`bin` или `bin\x64`) и графического установщика cuDNN 9, размер `ggml-cuda.dll` и время сборки.

## Сложности

Номер OQ-223 параллельно заняла сессия журналов — вопросы перенумерованы.

## Дальше

8.1.2 — замеры на RTX 3090; перед проектированием 8.1.3 нужны ответы автора на OQ-224 (только CUDA 13) и OQ-225 (llama.cpp набором DLL). Непроверенное из «Не сделано» — первым делом на машине с RTX 3090.
