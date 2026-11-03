---
title: Передача — слежение без повторов и ложных тревог (шаг 1.1.5, ISS-106)
---

# Передача — слежение без повторов и ложных тревог (шаг 1.1.5, ISS-106)

## Сделано

- Документы: [слежение](../../features/indexing/updates.md) (правило без корня и тревога только у наблюдаемых), [аудиты](../../features/knowledge-base/audits.md) (пропуск проекта без корня), [состояния окна](../../features/window/error-states.md) (новый раздел «Корня документации нет на диске»), [исследование нагрузки](../../knowledge/resource-efficiency.md), ISS-106 в [известных проблемах](../../backlog/issues.md) — «код готов, проверка ждёт общего прогона».
- Код (не собирался, не запускался, не коммитился):
  - `crates/bars-worker/src/worker/mod.rs`: поле `Runtime.docs_root_missing`, метод `Inner::note_docs_root`; `ensure_watching` не запускает слежение без папки корня и пишет в журнал один раз при переходе; `after_full_check` объявляет слежение молчащим и пишет «слежение молчало» только у наблюдаемого проекта.
  - `crates/bars-worker/src/handler/mod.rs`, `handler/audits.rs`: `WorkerHandler.schedule_blocked`, `schedule_docs_root_present`; тик расписания пропускает проект без папки корня, одна запись при смене состояния.
  - Тесты (написаны, не запускались): `worker/tests.rs` — `missing_docs_root_does_not_retry_watching_and_recovers`, `silent_watcher_of_unobserved_project_is_left_alone`; `handler/tests/audits.rs` — `schedule_skips_project_without_docs_root_and_notes_it_once`.
  - `cargo fmt` (rustfmt) применён к `worker/tests.rs`, `handler/tests/audits.rs`, `handler/audits.rs`, `handler/mod.rs`; rustfmt идёт и по дочерним модулям `handler/mod.rs`, поэтому в чужих незакоммиченных правках возможны мелкие сдвиги форматирования.

## Не сделано

- Окно: повтор «каждые 10 с» оказался подтверждением аренды `project_observe` (так задумано), а запуск слежения повторял воркер; `ui/src/lib/app.svelte.ts` не менялся.
- Кнопки «выбрать корень» прямо в строке состояния дерева нет: дерево без корня показывает прежнюю строку из снимка диска; выбор корня — существующее действие `chooseDocsRoot`. Если автору нужна кнопка в строке — отдельный шаг.
- Ничего не собрано и не прогнано: нужен общий `scripts/check.ps1`.

## Сложности

- В `worker/mod.rs` лежат чужие незакоммиченные правки; правки сделаны точечно.

## Дальше

После разрешения автора — `pwsh -File scripts/check.ps1`, затем коммит кода шагов вместе; ISS-106 в таблицу исправленных.
