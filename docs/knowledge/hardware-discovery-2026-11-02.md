---
title: Определение железа из Rust — Windows и macOS
---

# Определение железа из Rust — Windows и macOS

Статус 02-11-2026: исследование 8.3.2 завершено. Windows-путь проверен отдельным Rust-спайком на одной машине с RTX 3090; macOS изучена по первичным API и Rust-bindings, без сборки и физической проверки на Mac. Продуктовый детектор и экран железа относятся к 8.3.7. Предлагаемые API и структура данных — материал для проектирования, не утверждённый контракт.

## Предмет и метод исследования железа

Основание — [IDEA-19](../backlog/ideas.md), OQ-208, OQ-209, OQ-211 и Т-3, Т-17. Исследование продолжает [сравнение способов подбора моделей](model-hardware-presets-2026-11-01.md). Нужны имя и тип каждой GPU, физическая и доступная RAM, выделенная и общая память, бюджет процесса, NPU, версия драйвера и отдельные сведения о CUDA. Найденное устройство, загружаемая библиотека и исполнение конкретной модели — три разные проверки.

Порядок: сверить действующие загрузчики bars и первичные API Microsoft/Apple/NVIDIA; проверить Windows отдельным Rust-спайком без моделей и изменений настроек; сравнить с независимыми системными сведениями; измерить стоимость обнаружения; описать macOS и недоступные физические проверки; передать ограничения в проектирование. Число ступеней, резервы памяти и скорость моделей этим методом не устанавливаются.

## Вывод для будущего детектора

Универсального числа «память компьютера для моделей» нет. На Windows достаточно начать с Win32 RAM + DXGI + DXCore; D3D12 уточняет UMA, отдельный запрос NVIDIA — возможности драйвера. На macOS нужны Foundation/sysctl + Metal, а для Neural Engine — Core ML с проверкой доступности API. Ни один из этих путей не требует Python, запуска модели или установки CUDA ради самого определения железа.

Хранить раздельно физическую ёмкость, изменяемый бюджет процесса и фактическое исполнение модели. Тип GPU и топология памяти — разные признаки: `IsIntegrated=false` и `UMA=false` на проверенной RTX совпали, но из одного нельзя выводить другое на всех платформах. Ошибка запроса — неизвестное значение с причиной, а не ноль, отсутствие устройства или запрет всей рекомендации.

## Windows: RAM, GPU и топология памяти

| Данные | API из Rust | Смысл и ограничение |
|---|---|---|
| RAM, доступная физическая память, commit | `GlobalMemoryStatusEx`, `MEMORYSTATUSEX` | `ullTotalPhys` — доступная ОС физическая RAM; `ullAvailPhys` — меняющееся доступное количество. `ullTotalPageFile/ullAvailPageFile` — commit limit/headroom, не физическая RAM и не буквальный размер pagefile |
| Все графические адаптеры | `CreateDXGIFactory1`, `IDXGIFactory1::EnumAdapters1`, `GetDesc1` | Имя, vendor/device ID, LUID, три 64-битных размера памяти. Программные адаптеры исключаются из аппаратной рекомендации |
| Встроенная/дискретная GPU и версия драйвера | `DXCoreCreateAdapterFactory`, `GetAdapterByLuid`, `GetProperty` | `IsHardware`, `IsIntegrated`, `DriverVersion`. Связь с DXGI по LUID, а не имени или порядку; версия драйвера не равна версии CUDA |
| Общая архитектура памяти | `D3D12CreateDevice`, `CheckFeatureSupport(D3D12_FEATURE_ARCHITECTURE1)` | `UMA`, `CacheCoherentUMA`; запрос для узла 0 в спайке. Для linked multi-node нужен отдельный обход узлов |
| Бюджет GPU-памяти | `IDXGIAdapter3::QueryVideoMemoryInfo` | `Budget`, `CurrentUsage` относятся к вызывающему процессу; `AvailableForReservation` — возможность резерва, не свободная VRAM |

Основания: [GlobalMemoryStatusEx](https://learn.microsoft.com/en-us/windows/win32/api/sysinfoapi/nf-sysinfoapi-globalmemorystatusex), [MEMORYSTATUSEX](https://learn.microsoft.com/en-us/windows/win32/api/sysinfoapi/ns-sysinfoapi-memorystatusex), [поля дескриптора DXGI](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/ns-dxgi1_2-dxgi_adapter_desc2), [свойства DXCore](https://learn.microsoft.com/en-us/windows/win32/api/dxcore_interface/ne-dxcore_interface-dxcoreadapterproperty), [архитектура D3D12](https://learn.microsoft.com/en-us/windows/win32/api/d3d12/ns-d3d12-d3d12_feature_data_architecture1), [параметры бюджета](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_4/ns-dxgi1_4-dxgi_query_video_memory_info).

`DedicatedVideoMemory` и `DedicatedSystemMemory` описывают отдельные выделения; `SharedSystemMemory` — верхняя граница системной памяти для адаптера. Наличие shared-поля у дискретной GPU не превращает её в UMA. Для UMA local-сегмент включает общую память, non-local может быть нулём; нельзя считать local всегда отдельной VRAM. Отдельные резервы ОС и модели ещё потребуют измерений. Эти различия объясняет [стратегия управления памятью D3D12](https://learn.microsoft.com/en-us/windows/win32/direct3d12/memory-management-strategies).

`Win32_VideoController.AdapterRAM` имеет тип `uint32`; это непригодный основной источник для современных больших GPU. WMI полезен как независимая сверка имени и драйвера, но не как источник численного бюджета. Тип свойства подтверждает [Microsoft WMI](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-videocontroller).

DXGI-индекс, CUDA ordinal, Vulkan physical-device index и индекс ggml не взаимозаменяемы. Для дальнейшей интеграции нужен backend-specific mapping по поддерживаемым LUID/UUID/PCI-идентификаторам и проверка совпадения. В bars сейчас ONNX явно использует DXGI device 0 (`models/builtin.rs::gpu_providers`), GGUF — штатный выбор llama.cpp (`models/gguf.rs::load`). Просто обнаружить самую большую GPU недостаточно, чтобы начать считать на ней.

## Windows: обнаружение NPU и неполных данных

[DXCore enumeration](https://learn.microsoft.com/en-us/windows/win32/dxcore/dxcore-enum-adapters) различает графические и compute-адаптеры. Запрашивать NPU следует по `DXCORE_HARDWARE_TYPE_ATTRIBUTE_NPU`; generic-ML/core-compute признаки сами по себе не доказывают NPU. Для старых ОС/драйверов новые атрибуты могут не поддерживаться. Успешный пустой список означает «API не перечислил NPU в текущей конфигурации», а не доказанное отсутствие чипа. Отказ фабрики или запроса — отдельное состояние `unknown/unsupported`.

Runtime-agnostic hardware-type GUID позволяют перечислять и некоторые устройства без Direct3D user-mode driver; ограничиваться списком D3D12 graphics нельзя. Точные GUID и назначение подтверждает [таблица атрибутов Microsoft](https://learn.microsoft.com/en-us/windows/win32/dxcore/dxcore-adapter-attribute-guids). В `windows 0.62.2` этих новых констант нет: в спайке использованы значения из таблицы, без объявления отсутствующих COM-интерфейсов.

Для 8.3.7 предлагается DXGI как основной графический список, DXCore как дополнительный источник свойств/NPU с проверкой наличия DLL и функций во время выполнения. Базовая документация DXCore указывает Windows 10 build 18936; это не гарантия новых NPU-атрибутов на всех Windows 10. Спайк статически импортирует DXCore и проверен только на Windows 11 build 26300; переносить этот способ импорта в продукт без ограничения минимальной ОС нельзя. SetupAPI/PnP может дать дополнительное имя устройства без рабочего драйвера, но универсальная классификация NPU по PnP здесь не проверена. Распознавание по слову «AI» или имени CPU отвергнуто.

Решение OQ-211 сохраняется: найденный NPU только показывается; его память и TOPS не участвуют в выборе исполняющего устройства для модели bars.

## Windows: драйвер NVIDIA, Toolkit и рабочий CUDA backend

| Уровень | Проверка | Чего не доказывает |
|---|---|---|
| Драйвер | `nvcuda.dll` из System32; `cuDriverGetVersion`, отдельно `cuInit` и `cuDeviceGetCount` | Наличие установленного Toolkit, cuDNN или CUDA-сборки bars |
| Установка Toolkit | Кандидаты `CUDA_PATH`, стандартные версии в Program Files; `version.json`, файлы cudart/cuBLAS | Какую DLL реально загрузит процесс и разрешатся ли её зависимости |
| Загруженный runtime | Контролируемая загрузка точной cudart и `cudaRuntimeGetVersion`; реальные пути/версии библиотек | Исполнение конкретной ONNX/GGUF-модели и достаточную память |
| Модель в backend | Открытие сессии выбранной сборки и корректный результат на фиксированном вводе | Гарантированный выигрыш скорости и памяти на произвольной нагрузке |

`cuDriverGetVersion` возвращает уровень CUDA API драйвера; это отдельная величина от пользовательской версии драйвера, например `616.92`. Код версии имеет вид `1000*major + 10*minor`. Факт установки подтверждается manifest, а не строкой CUDA в `nvidia-smi`. С CUDA 13 на Windows `cudaRuntimeGetVersion` может описывать runtime из пакета драйвера, поэтому даже он не универсальная версия установленного Toolkit. Основание — [NVIDIA Version Management](https://docs.nvidia.com/cuda/cuda-runtime-api/cuda_runtime_api/group__CUDART____VERSION.html).

Спайк загружает только системный `nvcuda.dll` с `LOAD_LIBRARY_SEARCH_SYSTEM32`; Toolkit DLL не исполняет, `PATH` не меняет, CUDA-контекст и модели не создаёт. Описывает найденные 12.6/12.8 как установочные кандидаты, `loadability=not_tested`. Полный путь будущей проверки runtime, cuDNN и совместимость с закреплённым ORT/ggml описывает [исследование устройств](inference-devices.md), раздел «CUDA в сборке bars»; определения в этом исследовании не принимают решение 8.1.3 за автора.

В продукте нельзя грузить случайную DLL из текущего каталога ради версии. Нужны явный доверенный путь, управляемый поиск зависимостей и сохранение причины отказа. Проверку драйвера/создания backend следует выполнять вне UI и замков модели; потенциально зависающий native вызов — кандидат для собственного ограниченного по времени helper-процесса. Таймаут нельзя реализовать уничтожением произвольного потока с живыми native-объектами.

## macOS: Metal, общая память и Neural Engine

| Сведения | Публичный путь | Ограничение |
|---|---|---|
| RAM | Foundation `NSProcessInfo.physicalMemory` или `sysctlbyname("hw.memsize")` | Физическая RAM; доступность и давление памяти — отдельные показатели |
| GPU | `MTLCopyAllDevices`, `name`, `registryID` | Все доступные Metal-устройства, а не только default GPU; registryID нужен для сопоставления в рамках системы |
| Общая память | `MTLDevice.hasUnifiedMemory` | Доступен с macOS 10.15; не выводить из имени Apple/Intel или объёма памяти |
| Бюджет Metal | `recommendedMaxWorkingSetSize`, `currentAllocatedSize` | Рекомендованный рабочий набор и ресурсы данного процесса/MTLDevice, не физическая VRAM и не глобально свободная память |
| NPU | Core ML `MLModel.availableComputeDevices`, экземпляры `MLNeuralEngineComputeDevice` | macOS 14+; перечисляет доступное для Core ML, не гарантирует размещение узлов модели на ANE |
| Драйвер | Версия/сборка macOS плюс доступные семейства/возможности Metal | Публичного аналога отдельной версии NVIDIA-драйвера для всех Metal GPU в исследовании не найдено; поле отдельного драйвера остаётся неизвестным |

Основания: [физическая RAM Apple](https://developer.apple.com/documentation/foundation/processinfo/physicalmemory), [sysctl в XNU](https://github.com/apple-oss-distributions/xnu/blob/main/libkern/libkern/sysctl.h), [список Metal GPU](https://developer.apple.com/documentation/metal/mtlcopyalldevices()), [общая память Metal](https://developer.apple.com/documentation/metal/mtldevice/hasunifiedmemory), [рабочий набор Metal](https://developer.apple.com/documentation/metal/mtldevice/recommendedmaxworkingsetsize), [учёт ресурсов Metal](https://developer.apple.com/documentation/metal/mtldevice/currentallocatedsize), [идентификатор GPU](https://developer.apple.com/documentation/metal/mtldevice/registryid), [доступные Core ML устройства](https://developer.apple.com/documentation/coreml/mlmodel/availablecomputedevices-42uzt), [Neural Engine](https://developer.apple.com/documentation/coreml/mlneuralenginecomputedevice).

Swift `MLComputeDevice.allComputeDevices` также доступен с macOS 14; Rust следует связывать с Objective-C API, а не напрямую со Swift enum. Кандидаты — `objc2-metal`/`objc2-foundation` и `objc2-core-ml` с нужными feature gates, `Retained`/autorelease pool и проверкой доступности классов/селекторов. [Rust-bindings Metal](https://docs.rs/objc2-metal/latest/objc2_metal/) и [Neural Engine](https://docs.rs/objc2-core-ml/latest/objc2_core_ml/struct.MLNeuralEngineComputeDevice.html) доступны; это проверка описанных интерфейсов, не компиляционная приёмка bars на Mac. Новые macOS-зависимости в workspace не добавлены.

Для Apple Silicon RAM и память GPU — один физический пул. Ограничение комплекта должно учитывать суммарные CPU/GPU allocations и меньший применимый бюджет, без сложения RAM и Metal working set. Для Intel Mac с отдельной GPU нельзя выдавать `recommendedMaxWorkingSetSize` за точную установленную VRAM. Если публичный подтверждённый источник физического объёма не выбран, поле остаётся неизвестным; парсинг `system_profiler`/IORegistry не принят как основной путь. Энергосбережение (`isLowPower`) не заменяет проверку общей памяти.

Доступную RAM на Mac нельзя определять как `free_pages` без учёта reclaimable/compressed памяти. Для дальнейшей проверки — `host_statistics64(HOST_VM_INFO64)` вместе с размером страницы, отдельно memory-pressure уведомления; формула переносимого безопасного headroom пока не установлена. Первичные сведения — [host_statistics64 в XNU](https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/kern/host.c) и [memory pressure](https://github.com/apple-oss-distributions/xnu/blob/main/doc/vm/memorystatus_notify.md). На старой macOS отсутствие нового Core ML API не означает отсутствие ANE; отображать неизвестность. Современный CUDA-путь на Mac не предлагается: [CUDA 10.2 была последней с поддержкой macOS](https://docs.nvidia.com/cuda/archive/10.2/cuda-toolkit-release-notes/index.html).

## Проверка Rust-спайка на Windows

Сырые данные: [20 отдельных запусков](hardware-discovery-2026-11-02/windows-runs.jsonl), [независимые сведения и SHA-256](hardware-discovery-2026-11-02/independent.json), [время по стадиям](hardware-discovery-2026-11-02/timing-summary.json). Машина: Windows 11 Pro 10.0.26300, i7-11700KF, RTX 3090, установлено 32 ГиБ RAM; Rust 1.99.0, `windows =0.62.2`. Бинарник debug, процессы отдельные, ОС и файловый кэш не очищались; все 20 запусков завершились.

| Наблюдение | Результат | Вывод |
|---|---|---|
| RAM | ОС видит 31,891 ГиБ, доступно 8,442–8,690 ГиБ | Установленная ёмкость и доступный запас — разные поля |
| RTX 3090 | DXGI dedicated 23,752 ГиБ, shared max 15,945 ГиБ; NVML через nvidia-smi — 24 ГиБ | Не скрывать источник; значения разных API не подменять друг другом |
| WMI | `AdapterRAM=4293918720`, около 4 ГиБ | Число не описывает ёмкость RTX; не использовать его для подбора |
| Классификация | DXCore hardware=true, integrated=false; D3D12 UMA=false | Эти признаки на данном устройстве согласованы |
| Программная GPU | Microsoft Basic Render Driver, hardware=false/software=true | Нужна фильтрация, иначе список железа вводит в заблуждение |
| Бюджет | local 23,002 ГиБ, usage спайка 0 | Это бюджет спайка, не измерение использования bars или всех процессов |
| NPU | Успешный пустой список DXCore | На этой машине NPU не перечислен; положительный путь не проверен |
| CUDA | Driver API 13040, init/count=0, устройств 1; Toolkit manifest 12.6.3 и 12.8.0 | Возможности драйвера и установленные версии Toolkit различаются; рабочий backend bars не проверен |

| Стадия, миллисекунды | Медиана | p95 | Максимум |
|---|---:|---:|---:|
| Весь native-снимок | 246,05 | 257,82 | 356,56 |
| RAM | 0,05 | 0,05 | 0,09 |
| DXGI + D3D12 + свойства | 231,14 | 244,14 | 342,81 |
| DXCore списки | 0,03 | 0,03 | 0,05 |
| CUDA driver API | 11,39 | 12,39 | 13,02 |
| Каталоги Toolkit | 0,68 | 1,18 | 2,96 |

Время измерено внутри процесса, без запуска exe, независимой WMI-сверки и сериализации/печати итогового JSON; общее время включает создание DXCore factory. Медиана — среднее двух центральных значений, p95 — nearest rank. Стадии выполняются последовательно; сумма медиан не обязана равняться общей медиане. Внешняя нагрузка не контролировалась; это стоимость конкретного способа обнаружения на одной машине, не SLA и не сравнение GPU. Основное время — объединённая DXGI/D3D12-стадия; доля создания D3D12 отдельно не измерена.

Для будущего воркера вывод: холодную инвентаризацию кэшировать и обновлять по явному запросу/смене устройств; не выполнять её при каждом движении слайдера или частом `models_info`. Изменяемую память перепроверять отдельно перед применением. Снимок helper-процесса годится для инвентаря, но его `CurrentUsage` не является usage процесса воркера; бюджет и allocations для рекомендации нужно измерять в нужном процессе/backend.

## Воспроизведение и границы спайка

Из корня репозитория, Rust и MSVC доступны в PATH; все build artifacts — в единственном разрешённом кэше:

```powershell
$env:PATH = "$env:USERPROFILE/.cargo/bin;$env:PATH"
$env:CARGO_TARGET_DIR = 'C:/bt/bars'
cargo build --locked --manifest-path scripts/hardware-probe/Cargo.toml
pwsh -File scripts/hardware-probe/collect.ps1 -Runs 20
```

`collect.ps1` перезаписывает три файла наблюдений этого исследования; предыдущую серию перед повтором нужно сохранить, если она ещё нужна. На каждый собственный probe установлен срок 10 секунд; при превышении останавливается только этот дочерний процесс. Системные настройки, профили, рабочий воркер, чужие процессы и модели не меняются. Существующая транзитивная зависимость workspace `windows 0.62.2` использована в отдельном Cargo-проекте, версия закреплена и lock сохранён; основной workspace не изменён. Источник API — Microsoft windows-rs, а не сторонний детектор с собственной базой названий GPU.

Спайк проверяет Windows API и стоимость инвентаря. Он не является переносимым продуктовым детектором: macOS-ветки нет, DXCore импортируется статически, только node 0, сведения Toolkit берутся из ограниченных стандартных каталогов, ошибки чтения установки не классифицированы, runtime/cuDNN не загружаются. Положительный NPU, iGPU/UMA, гибридный ноутбук, AMD/Intel GPU, eGPU, RDP/VM, машина без NVIDIA, отключённый/удалённый драйвер и старые ОС физически не проверены.

## Передача в проектирование и реализацию

Предложение структуры будущего снимка: источник и время каждого наблюдения; отдельные `gpus[]` и `npus[]`; тип устройства и тип памяти; физические размеры и бюджеты в байтах; состояние `known/unknown/unsupported/error` с причиной; версия драйвера, версия драйверного CUDA API, установочные кандидаты и реально загруженный runtime отдельно. Идентификаторы нужны для сопоставления внутри машины; серийные номера/имя пользователя/полный PnP inventory для рекомендации не нужны. Это предложение 8.3.5, не новая настройка или протокол.

| Следующий этап | Что передаётся |
|---|---|
| 8.3.3 | Раздельные RAM/VRAM/UMA бюджеты; измерять обе модели, контекст, индекс, конкуренцию и задержку освобождения ISS-79; размер файла не равен памяти |
| 8.3.5 | Неизвестность допустима; shared RAM не прибавляется к физической RAM; явное основание рекомендации; нет автоматической смены embed по снимку |
| 8.3.7 | Платформенные API и Rust-путь; изоляция медленного обнаружения, корректное сопоставление адаптеров; матрица физических проверок из раздела о границах |
| 8.1.3–8.1.4 | Три уровня CUDA и причина отказа; установленная 12.x не выдаётся за совместимость с будущей 13.x-сборкой |

Сохранность Т-25, ручной выбор, OQ-212…214 остаются в [описании встроенных моделей](../features/models/local-models.md). Закрытие исследования не означает готовность слайдера, поддержку NPU-инференса, принятую схему CUDA или физическую приёмку macOS.
