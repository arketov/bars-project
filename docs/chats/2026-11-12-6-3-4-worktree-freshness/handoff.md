---
title: Сессия 12-11-2026 — шаг 6.3.4, актуальность дельты worktree
---

# Шаг 6.3.4 — актуальность дельты: слежение за worktree и изменения владельца

Предыдущий handoff по теме: [дельта worktree](../2026-11-12-6-3-2-worktree-delta/handoff.md). Контракт — [worktrees.md](../../features/projects/worktrees.md), пункт 4 «Порядка реализации». Код написан без проверки сборкой: сборки, `cargo`, тесты и `check.ps1` не запускались; сигнатуры и вызывающие проверены чтением.

## Сделано

- `DocQueue` (`worker/doc_queue.rs`): ключ записи — `DocTarget` (`Project(путь)` / `Worktree{владелец, имя}`) и путь. Методы принимают `impl Into<DocTarget>`, вызывающие с `&Path`/`&PathBuf` не менялись. `Ready.project` стал `Ready.target`. Добавлены `take_ready`, `remove_worktrees`, `remove_worktree`; `counts` и список окна считают только проект. Наблюдатель задания собирает `written` — пути, которые задание записало или удалило.
- Новый `worker/wt_watch.rs`: состояние слежения по ключу `(владелец, имя)` (аренда MCP — час, аренды окон — 30 с, `ProjectWatcher` на корне вида), `ensure_wt_watching` (только при аренде и состоянии дельты не «нет»), `on_wt_event` (отбрасывает события при «нет»; `Rescan`/`Failed` — полная сверка), `touch_worktree_files` и `submit_worktree_docs` (пауза Т-41 в общей очереди, затем сверка путей в очередь дельт), `owner_changed` (сверка путей каждой живой дельты владельца: `Watch` при аренде, иначе `Periodic`), `cancel_wt_watching`, `expire_wt_observers`. Публичное: `Worker::observe_worktree_mcp`, `observe_worktree_window`, `edit_worktree_files` — для 6.3.5–6.3.6.
- `mod.rs`: поле `wt_watch`; таймер документов разводит цели; `process` после `apply` публикует пути владельца (`rebuild` и `full_check` — «все»); `Worker::cancel` зовёт `cancel_deltas`. `delta.rs`: `request_delta` поднимает слежение, `drop_delta`/«нет» снимают его и записи очереди, `cancel_deltas` — слежение и записи worktree. `activity.rs`: аренды `pub(super)`, истечение аренд W вместе с проектными.
- Подпись `owner_digest` после перезапуска сверяет `restore_delta` из 6.3.2 (полная сверка `Periodic`); в 6.3.4 к ней добавлен только тест.
- Тесты (не запускались): `worker/tests/wt_watch.rs` (`edit_in_worktree_is_found_after_pause`, `owner_edit_missing_in_worktree_does_not_change_view`, `watch_events_are_dropped_while_delta_state_is_none`, `restart_with_changed_owner_digest_resyncs_the_delta`, отмена проекта, аренда окна, правка из окна, приоритет без аренды) и пять тестов ключа-цели в `doc_queue.rs`.
- Документы: `worktrees.md` (раздел «Как это устроено в воркере (шаг 6.3.4)», таблица проверок, жизненный цикл), `document-queue.md`, `worker.md`.

## Не сделано

- Ничего не собиралось и не запускалось: возможны ошибки компиляции, `clippy`, `cargo fmt` (в новых тестах `doc_queue.rs` есть строки длиннее 100 знаков).
- Вызовы `observe_worktree_mcp`/`observe_worktree_window` из MCP и окна — шаги 6.3.5–6.3.6; пока слежение за W включают только тесты.
- Выдача поиска по виду после правки в W и у владельца (`edit_in_worktree_is_found_after_pause` на уровне поиска) — вместе с 6.3.3; здесь проверено на уровне дельты.
- Хвостовая проверка (`arm_tail`) для W не заведена: потерянные события ловят `Rescan`, часовая проверка и сверка при перезапуске.

## Сложности

- Правка владельца и сверка по путям: публикуются только документы, у которых `document_done` пришёл без ошибки; прерванная переиндексация (`rebuild`) ничего не публикует, так как индекс не подменён.
- Записи очереди документов для W удаляются при передаче в очередь дельт (не через `finish`): сверка дельты не использует `taken`; правка во время сверки даёт новую запись с новой паузой.
- `Worker::cancel` (остановка индексации проекта) теперь забывает состояния дельт его worktree; «готова» вернёт проверка старта/часовая или следующее обращение. Если автор считал «остановкой» только остановку воркера, эту строку в `Worker::cancel` стоит убрать.
- Файл `mod.rs` и `delta.rs` правились в одном checkout с параллельной сессией 6.3.3; пересечения по строкам не было.

## Дальше

- 6.3.5: вызвать `Worker::observe_worktree_mcp` на каждом MCP-вызове с `wt`; 6.3.6: `observe_worktree_window` при показе worktree и `edit_worktree_files` при сохранении документа в виде.
- Первым делом при разрешённой сборке: `cargo fmt`, `clippy`, тесты `bars-worker` (`worker::tests::wt_watch`, `worker::doc_queue`, `worker::tests::delta`).
