---
title: Передача сессии — проект редактора и исследование CodeMirror (22-10-2026)
---

# Передача сессии — проект редактора и исследование CodeMirror

## Сделано

- [Проект редактора](../../features/knowledge-base/editor-design.md): команды `document_read`/`document_write`/`document_revision`, ревизия по хешу, атомарная запись, состояния, автосохранение, конфликт, закрытие, сценарии E-01…12.
- [Исследование CodeMirror 6](../../knowledge/editor-spike.md) и [ADR-0005](../../adr/0005-editor-codemirror.md) (proposed); [сырые замеры](measure.json). Стенд `npm run editor-spike --prefix ui`, замер `node ui/scripts/editor-spike-measure.mjs <out.json>`.
- Зависимости CodeMirror добавлены в `ui/package.json` (закреплены); вопросы OQ-122…126 в QUESTIONS.md; пункт 3.1.2 в PLAN.md отмечен, 3.1.1 — частично.

## Сделано позже в той же сессии (шаг 3)

- OQ-122…126 закрыты принятием рекомендаций автора (QUESTIONS.md), проект редактора обновлён.
- Команды записи: `crates/bars-app/src/document_io.rs` (+ `document_io_tests.rs`, 11 тестов), команды в `commands.rs`, `Worker::docs_root` в `bars-worker`, `document_io` подключён в три примера `bars-app`. Проверено: `cargo test -p bars-app --bin bars` — 22 ок, `cargo clippy -p bars-app --bin bars --tests --no-deps -- -D warnings`, `cargo fmt --check`.
- Сборка идёт только с окружением MSVC (`vcvars64.bat`), `CMAKE_GENERATOR=Ninja`, `VULKAN_SDK`, `LIBCLANG_PATH` и своим `CARGO_TARGET_DIR` (использовался `%TEMP%t`). Без них `llama-cpp-sys-2` падает на VS-генераторе.

## Шаг 4 (хранилище и компонент редактора)

- `ui/src/lib/editors.svelte.ts` (`EditorBuffer`, `editors`): состояния loading/clean/dirty/saving/saved/error/conflict/readonly, автосохранение через 1 с, одна запись одновременно, правка во время записи пишется следующей, повтор после ошибки. Тесты — `editors.test.ts` (9).
- `ui/src/lib/Editor.svelte`: CodeMirror 6, GFM, блоки кода по языку, `closeBrackets`, `markdownKeymap`, подсветка отрывка (снимается первой правкой), `scrollToLine`.
- `WorkspaceDocument.svelte`: редактор по умолчанию, глаз — рендер из буфера, строка состояния, полоса ошибки/конфликта/только чтения. `api.ts`: `documentRead/Write/Revision`; вне Tauri запись — мок.
- Проверено: vitest 315 из 315, `npm run check`, `npm run build`, ручной прогон в браузере (русский ввод, статус, снятие подсветки).

## Шаг 5 (помощники разметки)

- `ui/src/lib/editor-commands.ts`: 13 команд `editor.*`, `Tab`/`Shift+Tab` по таблице с выравниванием, автопара `*`, `Backspace` по паре; тесты — `editor-commands.test.ts` (16) и сквозной в `Workspace.test.ts` (`Ctrl+B` по коду клавиши KeyB).
- `Editor.svelte`: адаптер команд, `run(id)`; `WorkspaceDocument.svelte`: кнопка «Aa», панель, шпаргалка. `keybindings.ts`: у `editor.*` снята отметка «Этап 3.4».
- `editors.discard(buffer)` — закрытие без записи (пригодится шагу 6).
- Проверено: vitest 332 из 332, `npm run check`, в браузере кнопка и `Ctrl+B`. OQ-127 — размещение панели и «Ссылка» без выбора документа.

## Шаг 6 (конфликт, диск, закрытие)

- `editors.svelte.ts`: `checkDisk`, `keepMine`, `takeDisk`, `saveBeside`, `Editors.flush/checkAll`, слушатели `focus`/`blur`/`visibilitychange`, дописывание правок при закрытии последнего вида. Тесты — `editors.test.ts` (19).
- `unsaved.svelte.ts` (`confirmClose`, `unsavedPrompt`, `startFlushListener`), `UnsavedDialog.svelte`; закрытие вкладки (крестик и `Ctrl+W`) идёт через `confirmClose`. Тесты — `unsaved.test.ts` (7) и сквозной в `Workspace.test.ts`.
- Rust: `flush.rs` (`Flush`, `request`, 6 тестов), команда `flush_reply`, вызов в `tray::quit_app` и `restart_app`; `flush` подключён в три примера `bars-app`.
- Проверено: `cargo test -p bars-app --bin bars` — 28 ок, clippy (`--no-deps`) и fmt чисты; vitest 350 из 350, `npm run check`, `npm run build`.

## Не сделано

- Ответы автора на OQ-122…126 и утверждение проекта; продуктового кода нет (шаги 3–7 плана 3.4.1).
- IME, раскладки, установленный WebView2, `Tab` в таблице — не проверены.
- Проверки `npm test`/`check` не запускались: продуктовый код не менялся.

## Дальше

Нативная приёмка 3.4.1 в настоящей сборке: выход из трея с несохранённой правкой и конфликтом, перезапуск, русский ввод/IME (E-09), бюджеты E-12; затем 3.4.2 (вкладки, `Ctrl+O`, отдельное окно).
