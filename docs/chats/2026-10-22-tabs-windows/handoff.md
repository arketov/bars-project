---
title: Передача сессии — вкладки, Ctrl+O и отдельные окна (3.4.2)
---

# Передача сессии — вкладки, Ctrl+O и отдельные окна (3.4.2)

## Сделано
- Решения автора OQ-138…140 (рекомендации приняты) записаны в QUESTIONS.md.
- Устройство и проверки E-13…E-16 — [редактор документа](../../features/knowledge-base/editor-design.md), раздел про вкладки, `Ctrl+O` и отдельные окна. Руководство и таблица клавиш обновлены.
- Код: `crates/bars-app/src/doc_window.rs`, команды `document_window_open/target/close` и `open_in_main_window`, `flush.rs` (ответы всех окон), `DocumentWindow.svelte`, `editors.svelte.ts` (событие `bars://document-saved`), выбор по `Ctrl+O` в `workspace-commands.ts` и `CommandPalette.svelte`, пункт меню «Открыть в новом окне».
- Тесты UI: `document-windows.test.ts` (10), `DocumentWindow.test.ts` (4); весь vitest — 376 проходят.

## Не сделано
- Rust не собран и не прогнан: `bars-core` в общем дереве не компилируется (`audits/store.rs`, чужая правка). Один раз Rust-код `bars-app` доходил до компиляции и дал единственную ошибку (не подключён `Emitter`), она исправлена.
- Нативная приёмка в настоящем окне и установленной сборке (OQ-113), включая E-09, выход из трея и E-12, перенесённые из 3.4.1.
- Ничего не закоммичено: в дереве смешаны правки нескольких сессий в одних файлах.

## Сложности
Сборка идёт только с `CARGO_TARGET_DIR=C:/bars-build`, `CMAKE_GENERATOR=Ninja`, MSVC, `LIBCLANG_PATH`, `VULKAN_SDK`; полная перекомпиляция зависимостей занимает около 20 минут из-за параллельных сборок других сессий.

## Дальше
`cargo test -p bars-app --bin bars` и clippy, когда `bars-core` снова собирается; затем нативная приёмка и закрытие 3.4.2 в плане.
