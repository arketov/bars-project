---
title: Сборка bars — как собрать, проверить и что не проверяется
---

# Сборка bars

Как собирается Cargo workspace bars ([architecture/components.md](../architecture/components.md)) и какие проверки гоняются перед коммитом. Версии крейтов — [architecture/stack.md](../architecture/stack.md).

## Требования к машине разработчика bars

- Rust `1.99.0`: версия закреплена в `rust-toolchain.toml` вместе с компонентами `rustfmt` и `clippy`. Нижняя граница стека — 1.90 ([architecture/stack.md](../architecture/stack.md)); точный пин нужен, чтобы сборка не менялась от обновления stable.
- Windows: MSVC не старше 17.14. macOS: Xcode Command Line Tools.
- Сеть: `ort` скачивает ONNX Runtime с `cdn.pyke.io` при первой сборке `bars-core`.

## Каталог сборки вне OneDrive

Репозиторий лежит в OneDrive, а каталог `target/` с ONNX Runtime и Tauri занимает гигабайты. Поэтому `CARGO_TARGET_DIR` задаётся вне репозитория:

```bash
export CARGO_TARGET_DIR="$LOCALAPPDATA/bars/target"
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

## Что собирается в срезе 0

Все пять крейтов с реальными зависимостями из стека и заглушками вместо логики. Этим проверяются Т-3 (Rust, Tauri, fastembed-rs, без Python) и то, что тяжёлые зависимости линкуются вместе. `bars-app` собирал Tauri с пустой статической страницей; окно на Svelte пришло в срезе 5.

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
