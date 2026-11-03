---
title: Выбор корня документации при добавлении папки — handoff шага 3.1.1
---

# Выбор корня документации при добавлении папки — handoff шага 3.1.1

Шаг плана 3.1.1 (Т-7, Т-35, раунд 33, OQ-246). Сборки, тесты и проверки не запускались по запрету автора; код ждёт общей проверки.

## Сделано

- **Документы.** Контракт — [projects.md](../../features/projects/projects.md): разделы «Корень документации при добавлении папки» и «Создание проекта», правило хука. Руководство — [projects.md](../../manual/projects.md): убрано «Проект — это папка с вашим кодом», описан выбор «Вся папка» / «Папка `docs/`». Ключ `docs_root` в [settings.md](../../features/settings/settings.md) ссылается на контракт.
- **Решение по устройству.** Выбор пишется клиентом в `.bars/config.toml` до `register`; воркер существующий `config.toml` не трогает (`ensure_project_config`), поэтому протокол и воркер не менялись. Общая логика — `crates/bars-protocol/src/project_root.rs` (`RootChoice`, `prepare_project`, `create_project`, `initial_config_text`): в протоколе, потому что у `bars-cli` нет `bars-core`, а окну нужно то же.
- **CLI.** `bars-cli register [путь] --root docs|all`; без флага — умолчание (`docs/`, если есть, иначе вся папка). Только для проекта без `config.toml`.
- **Окно.** Tauri-команды `project_root_suggest` и `project_root_prepare` (`bars-app/src/commands.rs`, `main.rs`), диалог `ui/src/lib/ProjectRootDialog.svelte` («Что считать документацией?»), состояние и методы `rootChoice`, `askRoot`, `confirmRoot`, `cancelRoot` в `app.svelte.ts`; тот же диалог у «Добавить проект на новом месте…».
- **Создание проекта (Т-35).** `create_project`: корень (папка `docs/` только при выборе `docs/`), `index.md` (если нет файла с таким именем в любом регистре), `config.toml`; отказ, если `config.toml` уже есть. В окне и CLI отдельной команды «создать» пока нет — функция готова для неё.
- **Воркер.** `register` вместо `ensure_project_config` вызывает `bars_protocol::prepare_project(path, None)`. По решению автора OQ-253 (раунд 34) `session_start` регистрирует папку только при `.bars/config.toml` или `docs/`, иначе `None`; корень проверки — `docs_root` из `config.toml` или `docs`. `ensure_project_config` и ненужный `use std::fs` удалены из `worker/mod.rs`. Тест `session_start_skips_a_folder_without_docs_or_config` в `worker/tests.rs`: папка заметок хуком не регистрируется, явная регистрация даёт корень `.`, проект с `config.toml` регистрируется.
- **Тесты (написаны, не запускались).** Юнит-тесты в `project_root.rs`; `register_root_choice_is_written_before_registration` в `crates/bars-cli/tests/cli.rs`; `ui/src/lib/project-root.test.ts`.
- Уже зарегистрированные проекты не меняются: запись идёт только при отсутствии `config.toml`.

## Не сделано

- Обновление экрана «документации не найдено» (`docs/features/window/empty-states.md`, `NoDocs.svelte`): он называет корень `docs` по умолчанию — для старых проектов верно.
- Отдельная команда или пункт окна «Создать проект» (Т-35): вызывающего кода нет.

## Сложности

- Выбор записывается клиентом, потому что воркер был занят шагом 1.1.1; теперь воркер сам пишет умолчание.
- Побочное решение: `bars-cli` теперь трогает диск (пишет `config.toml`) — заголовок `lib.rs` обновлён.

## Риски (для проверки)

- **Обход при «вся папка».** `walk_docs` пропускает папки с точки (`.bars`, `.git`) и симлинки, `BARS_INSTRUCTION.md` в корне не индексирует; watcher (`worker/watcher.rs::is_relevant`) игнорирует события внутри папок с точки. **Не исключаются** `node_modules`, `target`, `vendor`, `dist`: `.md` из них попадут в индекс репозитория, выбранного как «вся папка». Для кода предложена `docs/`; решение о списке исключений или `.gitignore` — за автором. Событие самой папки `.bars` (без расширения) watcher считает значимым: возможна лишняя проверка, задание без изменений не создаётся (НФТ-7).
- Риск «хук регистрирует любую папку с `.md`» снят решением OQ-253: хук требует `docs/` или `.bars/`.
- Существующие тесты, регистрирующие папки без `docs/` через CLI, теперь получат корень `.`: проверить при общей проверке.
- В `ProjectRootDialog.svelte` нет Escape-теста; `project-root.test.ts` написан без запуска.

## Файлы кода, ждущие общей проверки (не закоммичены)

- `crates/bars-worker/src/worker/mod.rs`, `crates/bars-worker/src/worker/tests.rs` (в них могут быть и чужие правки)
- `crates/bars-protocol/src/project_root.rs` (новый), `crates/bars-protocol/src/lib.rs`
- `crates/bars-cli/src/lib.rs`, `crates/bars-cli/tests/cli.rs`
- `crates/bars-app/src/commands.rs`, `crates/bars-app/src/main.rs`
- `ui/src/lib/ProjectRootDialog.svelte` (новый), `ui/src/lib/project-root.test.ts` (новый), `ui/src/lib/api.ts`, `ui/src/lib/app.svelte.ts`, `ui/src/App.svelte`

`api.ts` и `app.svelte.ts` могут содержать и чужие правки: при коммите брать только свои хунки.

## Дальше

1. Общая проверка `scripts/check.ps1`, затем коммит кода.
2. Шаг 3.1.2 (тексты окна и подсказки модулей) и 3.1.3 (`BARS_INSTRUCTION.md`).
