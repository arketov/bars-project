---
title: Сборка bars — как собрать, проверить и что не проверяется
---

# Сборка bars

Как собирается Cargo workspace bars ([architecture/components.md](../architecture/components.md)) и какие проверки гоняются перед коммитом. Версии крейтов — [architecture/stack.md](../architecture/stack.md).

## Требования к машине разработчика bars

- Rust `1.99.0`: версия закреплена в `rust-toolchain.toml` вместе с компонентами `rustfmt` и `clippy`. Нижняя граница стека — 1.90 ([architecture/stack.md](../architecture/stack.md)); точный пин нужен, чтобы сборка не менялась от обновления stable.
- Windows: MSVC 17.14+, CMake/Ninja, Vulkan SDK (`VULKAN_SDK`) и LLVM для bindgen в `llama-cpp-sys-2` (`LIBCLANG_PATH`, например `C:\Program Files\LLVM\bin`). Всё ставится через winget: `LLVM.LLVM`, `Kitware.CMake`, `Ninja-build.Ninja`, `KhronosGroup.VulkanSDK`. Для нативной GGUF-сборки использовать Developer PowerShell последнего MSVC, `CMAKE_GENERATOR=Ninja`, `CMAKE_BUILD_PARALLEL_LEVEL=1`, короткий `CARGO_TARGET_DIR` (например, `C:/bars-build`). MSBuild с вложенным shader generator нестабилен на чистом каталоге. macOS: Xcode Command Line Tools; Metal не проверен.
- Сеть: `ort` скачивает ONNX Runtime с `cdn.pyke.io` при первой сборке `bars-core`.

## Каталог сборки вне OneDrive

Репозиторий лежит в OneDrive, а каталог `target/` с ONNX Runtime и Tauri занимает гигабайты. Поэтому `CARGO_TARGET_DIR` задаётся вне репозитория:

```bash
export CARGO_TARGET_DIR="C:/bars-build"
```

Путь в репозиторий не записывается: он у каждого свой.

## Проверки перед коммитом

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace
```

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

- **Непрерывной интеграции нет.** Решение автора 14-10-2026: репозиторий приватный, macOS-раннеры платные с множителем 10 (OQ-18), поэтому GitHub Actions не заведён. Проверки выше выполняются руками.
- **Сборка на macOS не проверена.** Это часть Т-4. Проверка — на реальном Mac.
- **`cargo audit` не запускается.** Без CI он не встроен ни в какой процесс; запускать его вручную можно, когда понадобится.
