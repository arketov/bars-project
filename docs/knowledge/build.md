---
title: Сборка bars — как собрать, проверить и что не проверяется
---

# Сборка bars — как собрать, проверить и что не проверяется

Как собирается Cargo workspace bars ([architecture/components.md](../architecture/components.md)) и какие проверки гоняются перед коммитом. Версии крейтов — [architecture/stack.md](../architecture/stack.md).

## Требования к машине разработчика bars

- Rust `1.99.0`: версия закреплена в `rust-toolchain.toml` вместе с компонентами `rustfmt` и `clippy`. Нижняя граница стека — 1.90 ([architecture/stack.md](../architecture/stack.md)); точный пин нужен, чтобы сборка не менялась от обновления stable.
- Windows: MSVC 17.14+, CMake/Ninja, Vulkan SDK (`VULKAN_SDK`; `scripts/build-installer.ps1` находит его сам, остальные команды ждут переменную) и LLVM для bindgen в `llama-cpp-sys-2` (`LIBCLANG_PATH`, например `C:\Program Files\LLVM\bin`). Всё ставится через winget: `LLVM.LLVM`, `Kitware.CMake`, `Ninja-build.Ninja`, `KhronosGroup.VulkanSDK`. Для нативной GGUF-сборки использовать Developer PowerShell последнего MSVC, `CMAKE_GENERATOR=Ninja`, `CMAKE_BUILD_PARALLEL_LEVEL=1`. MSBuild с вложенным shader generator нестабилен на чистом каталоге. macOS: Xcode Command Line Tools; Metal не проверен.
- git в `PATH`: `bars-protocol` берёт из него идентификатор сборки для сверки в `hello` ([протокол](../architecture/protocol.md)); без git сборка идёт, но проверка сборок молчит (`unknown`).
- Сеть: `ort` скачивает ONNX Runtime с `cdn.pyke.io` при первой сборке `bars-core`.

## Где хранятся артефакты сборки target

Локальный Vulkan SDK найден также в `C:/tools/VulkanSDK/1.4.363.0`. Штатный установочный скрипт проверяет `VULKAN_SDK`, `C:/VulkanSDK` и `C:/tools/VulkanSDK`; каталог `Lib` выбранного SDK добавляет в окружение линковщика текущего процесса. Это позволяет использовать готовые C++-артефакты, даже если их кэш сохранил старый путь к import library `vulkan-1.lib`. Глобальные переменные и установленный SDK скрипт не меняет.

Cargo без переопределений пишет в `target/` корня workspace, но на этой машине там ломается llama.cpp, поэтому каталог сборки задаётся явно — единственный кэш `C:\bt\bars`:

```powershell
$env:CARGO_TARGET_DIR = 'C:/bt/bars'
```

`scripts/build-installer.ps1` сохраняет заданный `CARGO_TARGET_DIR`; без него использует `<корень репозитория>/target`. EXE лежат в `<target>/debug/` или `<target>/release/`, NSIS — в `<target>/release/bundle/nsis/`; скрипт копирует установщик в `dist-installer/` либо указанный `-OutDir`. Это разные места: `target` — рабочие артефакты, выходной каталог — копия для передачи.

Если работающий Vite удерживает `rolldown-binding.win32-x64-msvc.node`, `npm ci` может завершиться EPERM при удалении зависимостей. Не останавливать чужой процесс: для уже установленных зависимостей `scripts/build-installer.ps1 -SkipNpmCi` сначала проверяет `npm ls --depth=0`, затем собирает UI. Если предыдущий `npm ci` успел удалить часть пакетов, восстановить `npm install --ignore-scripts --prefer-offline` и проверить неизменность `package-lock.json` перед сборкой. Флаг не разрешает менять версии зависимостей.

На машине разработки release-артефакты лежат в `target/` репозитория. Наличие папки не определяет, куда пишет следующий запуск: это задают `CARGO_TARGET_DIR` и текущий workspace. Не очищать общий Cargo target ради отдельного теста.

## Длинный путь генератора Vulkan на Windows

При сборке `llama-cpp-sys-2 0.1.160` внутри длинного пути workspace MSVC может остановиться с C1083 (`Invalid argument`) на `vulkan-shaders-gen-build/CMakeFiles/CMakeScratch/TryCompile-…/testCCompiler.c`. Такой сбой воспроизведён 23-10-2026 в `target/debug`: вложенный пробный проект CMake добавляет путь к рабочему каталогу. Кэш сборки один — `C:\bt\bars`: все сборки и проверки идут с `CARGO_TARGET_DIR=C:/bt/bars` (решение автора 23-10-2026). Каталог постоянный, его не удаляют и не очищают, чтобы не пересобирать llama.cpp с нуля. Других каталогов сборки (`C:\bt\<что-то ещё>`, `target\` через `subst`), логов, выгрузок и любых файлов вне проекта не создавать. Среда: MSVC (Developer PowerShell), `CMAKE_GENERATOR=Ninja`, `CMAKE_BUILD_PARALLEL_LEVEL=1`, Vulkan; системные настройки не менять и зависимости не переустанавливать. `cargo clippy --workspace --all-targets` собирает и пример `docking_probe`; ему нужна папка `ui/docking-dist` (`npm run probe:docking:build` в `ui/`), без неё макрос Tauri падает.

## Где хранятся временные файлы тестов и кэш моделей

| Данные | Место и жизненный цикл |
|---|---|
| Временные проекты, индексы и профили Rust-тестов | `tempfile::TempDir` в системном временном каталоге; на Windows обычно `%TEMP%` (`C:/Users/arketov/AppData/Local/Temp` на проверенной машине). Каталог удаляется при штатном уничтожении `TempDir`; аварийное завершение может оставить остатки |
| Изолированный профиль интеграционного стенда | `BARS_USER_HOME` указывает на домашнюю папку стенда внутри собственного временного каталога; настройки клиентов и данные bars создаются в ней. Рабочий профиль пользователя не использовать |
| Скачанные встроенные модели | `<домашняя папка>/.bars/models/`, на проверенной машине `C:/Users/arketov/.bars/models/`; это общий постоянный кэш, а не временные данные теста и не `.bars` проекта. Контракт — [local-models.md](../features/models/local-models.md#где-лежат-скачанные-модели) |

Аппаратные исследования используют один собственный временный каталог вне репозитория, существующие веса только для чтения без копий. После сохранения небольших доказательств убрать собственные временные данные, включая неуспешный выход; общие модели и чужие артефакты не удалять (решение автора OQ-107 в `QUESTIONS.md`). Тесты с настоящей моделью, описанные далее, читают готовый пользовательский кэш без скачивания; не все временные файлы проекта создаются через `TempDir`.

## Проверки перед коммитом

Одна команда запускает все проверки подряд и останавливается на первой красной (ISS-65):

```powershell
pwsh -File scripts/check.ps1            # Rust и окно
pwsh -File scripts/check.ps1 -SkipUi    # только Rust
pwsh -File scripts/check.ps1 -Jobs 2    # меньше заданий, если не хватает памяти
```

Шаги скрипта: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo build --workspace`; `cargo test --workspace --lib --tests --bins --no-fail-fast`; затем в `ui/` `npm run check` и `npm test`. Каждый cargo-шаг идёт с `-j 3` (число меняет `-Jobs`).

Почему не просто `cargo test --workspace`: при числе заданий по умолчанию линковщик Windows падает на примерах и тестах `bars-core` с `link.exe failed: exit code 1` без текста ошибки; на 8,9 из 31,6 ГБ свободной памяти это воспроизведено дважды (аудит внутренней части 23-10-2026, ВН-03). Причина не установлена, вероятная — нехватка памяти при одновременной линковке больших бинарников с отладочной информацией. С `-j 3` сборка проходит; `--lib --tests --bins` не собирает примеры, которые никто не запускает. Примеры с тяжёлыми стендами запускаются отдельно, своими скриптами из `scripts/`.

Если `CARGO_TARGET_DIR` не задан, скрипт сам ставит `C:/bt/bars` — единственный кэш сборки (раздел «Длинный путь генератора Vulkan на Windows»): в `target\` репозитория длинный путь ломает `llama-cpp-sys-2`. Так же по умолчанию поступают `probe-*.ps1`, `run-memory-acceptance.ps1`, `test-client-profile-isolation.ps1`, а `test-client-refresh.ps1`, `test-docs-delivery.ps1` и `test-mcp-disk-read.mjs` берут бинарники из `C:/bt/bars/debug`. Команды проверки идут с `--workspace`: сборка с `-p <крейт>` объединяет фичи иначе и собирает llama.cpp ещё раз в том же кэше. Скрипт также выставляет `LIBCLANG_PATH` (если есть `C:\Program Files\LLVM\bin`), `CMAKE_GENERATOR=Ninja` и `CMAKE_BUILD_PARALLEL_LEVEL=1`. На рабочем кэше `C:/bt/bars` скрипт проходит все шаги: 24-10-2026 зелёный прогон занял около 4 минут (тесты Rust 129 с, окно 68 с). Остановка на красном шаге проверена на падении `cargo test`. На чистом каталоге скрипт до конца не прогонялся.

Тесты с настоящей моделью и тяжёлый тест корпуса ниже в скрипт не входят.

## Бинарник примера занят другой сессией

Пример, запущенный другой сессией (например, `bench_quality.exe` в `C:\bt\bars\release\examples\`), нельзя пересобрать: линковщик падает с `LNK1104: cannot open file`. Чужой процесс не останавливать. Рабочий способ 24-10-2026: временная копия примера под другим именем — первая строка `//!` заменяется своим комментарием (через `include!` файл с внутренним doc-комментарием не собирается), остальное копируется как есть. До коммита удалить и копию, и её артефакты `C:\bt\bars\release\examples\<имя>*`. Повторение прогона на пересобранном бинарнике меняет его хеш в `config.json` прогона: продолжить старый прогон в той же папке результатов уже нельзя.

## Коммит при чужих правках в рабочем дереве

Несколько сессий агентов работают в одном checkout, и в общем файле (например, `QUESTIONS.md`) могут лежать чужие незакоммиченные правки рядом со своими. Если ханки перемежаются, `git add -p` агенту недоступен (интерактивный). Рабочий способ: собрать версию файла из `HEAD` плюс только свои вставки и положить её в индекс, не трогая рабочую копию — `git hash-object -w <файл>`, затем `git update-index --cacheinfo 100644,<blob>,<путь>`. Чужие правки остаются в рабочем дереве незакоммиченными. Перед коммитом проверить `git diff --cached` этого файла.

## Интерфейс окна: сборка и проверки

Окно — Svelte 5 + TypeScript + Vite в `ui/` ([architecture/components.md](../architecture/components.md)). Нужен Node.js 20 или новее (проверено на 24).

```bash
cd ui
npm install        # один раз
npm run build      # ui/dist: его встраивает Tauri при сборке bars
npm test           # vitest: логика дерева, форматирование, Markdown, настройки, согласие
npm run check      # svelte-check: типы TypeScript и Svelte
npm run dev        # просмотр в браузере на порту 1420: вместо воркера отвечает заглушка src/lib/mock.ts
```

`cargo build` и `clippy` не требуют `npm run build`: если `ui/dist/index.html` нет, сборочный скрипт `bars-app` кладёт туда заглушку «Интерфейс не собран». Настоящий интерфейс в `bars` попадает только после `npm run build` и пересборки `bars-app`.

Заглушка воркера выбирает сцену параметром адреса: `?scene=empty|indexing|consent|missing|provider|worker-down`; без параметра — обычный проект с тремя результатами.

## Тяжёлый тест поискового корпуса

`search_corpus` переэмбеддит всю текущую документацию. По наблюдению автора 18-10-2026 жёсткий CPU-прогон слишком долго блокировал процессор и был остановлен; это не ошибка корректности поиска. Тест запускается только явно (`#[ignore]`): `cargo test --release -p bars-core --test search_corpus -- --ignored --nocapture`. По умолчанию требует фактический DirectML, без тихого продолжения на CPU; пачка 1, кооперативный бюджет индексации 60 с между документами. На CPU — только отдельный явный `BARS_CORPUS_TEST_DEVICE=cpu`. Новые веса не скачиваются. Остановленный CPU-прогон не считать успешной приёмкой.

Холодная release-сборка `bench_quality` в `C:/bt/bars` занимает около 16 минут из-за llama.cpp; сразу после неё первый запуск может ответить «Device or resource busy», повтор проходит.

Для GraphRAG на готовом индексе используются сохранённые embeddings ([опыт на готовых embeddings](graph-embedding-2026-10-18.md), `scripts/graph-embedding-probe.mjs`): замер не требует полной переиндексации и не заменяет регрессию модели.

## Установщик Windows

`pwsh scripts/build-installer.ps1` собирает NSIS-установщик; подробности, выбор формата и проверка — [features/distribution/installer.md](../features/distribution/installer.md), [installer.md](installer.md). Нужна сеть при первой сборке (Tauri скачивает NSIS). Для просмотра и проверок ставить `@tauri-apps/cli` отдельно не нужно: он в `devDependencies` пакета `ui/`.

## Состав сборки и особенности зависимостей

Cargo workspace из семи крейтов (`crates/*`: `bars-app`, `bars-cli`, `bars-core`, `bars-graph`, `bars-mcp`, `bars-protocol`, `bars-worker`); их роли — [architecture/components.md](../architecture/components.md). Сборка вместе проверяет Т-3: Rust, Tauri, fastembed-rs и встроенный llama.cpp линкуются в одном workspace, Python нет.

Фича `directml` у fastembed включается только на Windows (`cfg(windows)`), потому что на macOS её нет.

Для выбора DirectML `bars-core` на Windows зависит от крейта `ort` напрямую, с той же версией `=2.0.0-rc.13`, что у fastembed.

## Тесты со встроенной моделью

Тесты `crates/bars-core/tests/builtin_embedder.rs` гоняют настоящую bge-m3: векторы на CPU и на DirectML, порядок векторов после сортировки по длине, индексация маленького проекта. Они берут модель только из `~/.bars/models` и ничего не скачивают: если модели там нет или в окружении задан `HF_HOME`, тест печатает «пропущено» и проходит. Поэтому «все тесты зелёные» на машине без модели не говорит про bge-m3. Прогон занимает около 30 секунд:

```bash
cargo test -p bars-core --test builtin_embedder -- --nocapture --test-threads=1
```

Остальные тесты индексации используют подделку модели из хешей и модель не требуют.

## Чего нет и что не проверено

- **Непрерывной интеграции нет.** Решение автора 14-10-2026: репозиторий приватный, macOS-раннеры платные с множителем 10 (OQ-18), поэтому GitHub Actions не заведён. Проверки запускаются руками через `scripts/check.ps1`.
- **Сборка на macOS не проверена.** Это часть Т-4. Проверка — на реальном Mac.
- **`cargo audit` не запускается.** Без CI он не встроен ни в какой процесс; запускать его вручную можно, когда понадобится.
