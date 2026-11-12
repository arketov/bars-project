---
title: Сессия 12-11-2026 — шаг 6.3.1, обнаружение worktree и разрешение цели
---

# Шаг 6.3.1 — обнаружение worktree и разрешение цели

Предыдущий handoff по теме: [фильтр проектов](../2026-11-12-6-2-3-projects-filter/handoff.md) (оставил шов для совпадения по worktree). Контракт — [worktrees.md](../../features/projects/worktrees.md), пункт 1 «Порядка реализации». Код написан без проверки сборкой: сборки, `cargo`, тесты и `check.ps1` не запускались; сигнатуры и вызывающие проверены чтением и поиском по репозиторию.

## Сделано

- `bars-core::worktree` (новый): `read_entries` (записи `.git/worktrees/*`: `gitdir`, `HEAD`, `locked`, `commondir`; состояния живой / пропавший / оторванный), `repository_of` (репозиторий проекта и смещение; файл `.git` по пути — worktree у проекта нет), `locate` (рабочая папка → живая запись; бывшая `main_checkout_of_worktree` + проверка живости), `linked_for_owner` (живые с корнем вида, «без вида» отбрасываются), `LinkedWorktree`, помощники путей. Тесты ядра: разбор, пропавшие/оторванные (случай `api-avatar`), подмодуль и bare, корень вида со смещением.
- `bars-core::indexing::nested`: `Area.worktree`, `WorktreeDir`, `publish_worktrees(owners, dirs)`, `areas_with`; `publish` учитывает папки worktree, так что обход, дерево, слежение и ссылки каждого охватывающего проекта их исключают. Тесты: исключение у каждого охватывающего проекта, возврат после удаления записи.
- Реестр (`registry.rs`): `resolve_cwd`/`CwdTarget`/`WorktreeRef`, `project_for_cwd` — оболочка, `discover_root` возвращает `Option<Discovered>` (`None` для живого worktree без зарегистрированного владельца; поле `worktree`). Тесты worktree переписаны и дополнены (владелец по `.worktree`, `.claude/worktrees`, вне проекта; подпроект с `.git`-каталогом; смещение владельца; мёртвая запись; хук).
- Воркер: `worker/worktrees.rs` — список `LinkedWorktree` по владельцам в памяти, `discover_worktrees` (один раз на репозиторий, ленивые тоже, без заданий и дельт), `WtSelector`, `Worker::find_worktree` (с одним перечитыванием при промахе), `ViewSelector`, `WtSource`. Триггеры: старт, часовой таймер, `session_start`, первая аренда окна, регистрация проекта, новые найденные подпроекты; `remove_entry` забывает worktree. `session_start` не регистрирует ни копию, ни основной checkout. `ProjectSnapshot.worktrees`; `nested_projects` папки worktree не показывает как вложенные проекты.
- Протокол: `Target.wt` (+ `Target::wt()` без пустой строки), `ErrorKind::WorktreeNotFound` (−32020), `WorktreeStatus`/`WorktreeDeltaStatus`/`DeltaState`/`WorktreeBrief`, `StatusOutput.worktrees`, `ProjectInfo.matched_worktrees`/`WorktreeMatch`. Все литералы `Target` в крейтах получили `wt: None`.
- Обработчик: `resolve_view` (`wt="."`, имя, путь, умолчание из `cwd`, правила явного `project`; `resolved_wt`, `wt_source` в `applied`), `resolve` — оболочка; ошибка `errors::worktree_not_found`; `status` отдаёт WORKTREES (`current`, только названный `wt`; `delta.state` всегда `none`); `projects` с `filter` заполняет `matched_worktrees`; `project_filter::matching` совпадает и по worktree.
- MCP/CLI: часть WORKTREES в `bars_status`, строки `matched_by: worktree`; человеческий вывод `bars-cli status` и `projects`. Параметра `wt` в схемах инструментов и `--wt` нет — шаг 6.3.5.
- Тесты воркера: `worker/tests/worktrees.rs` (обнаружение без дельты, хук, исключение и возврат папки, промах `wt` и перечитывание, ленивый владелец), `handler/tests/worktrees.rs` (`wt="."`, `worktree_not_found` с ветками и `data`, проект без worktree, правила явного `project`, WORKTREES, фильтр), рендер MCP, фильтр проектов.
- Документы: `worktrees.md` (статус, раздел реестра, хук, `publish_worktrees`), `projects.md`, `protocol.md` (цель вызова, таблица ошибок, `status`), `worker.md`, `clients.md`, `manual/projects.md`. Правило OQ-289/OQ-291 «worktree вне проекта — по основному checkout» заменено общим правилом вида в коде и в спеке.

## Не сделано

- Ничего не собиралось и не запускалось: возможны ошибки компиляции, `clippy` и `cargo fmt` (часть строк длиннее ста знаков).
- Запрос первого обращения к виду из `session_start` и сверка по тайм-ауру — вместе с дельтой (6.3.2); `delta.state` сейчас всегда `none`.
- Поиск и чтение по виду (`resolve` вид отбрасывает), `wt`/`--wt` в MCP и CLI, `session_info` с worktree, окно и `tree_read` с видом `worktree` в `nested`; ввод `wt` в историю MCP (список разрешённых полей `_mcp`) — шаги 6.3.3–6.3.6.
- Тест «обнаружение при `git worktree add` из окна» и ручная проверка на `tmc-opovechenie` — нужна сборка.

## Сложности

- Спека называет `nested::publish_worktrees(dirs)`; хранилище одно на процесс (как у областей), и параллельные воркеры тестов затирали бы друг друга, поэтому сигнатура — `(owners, dirs)`: заменяется список только названных владельцев.
- `discover_root` теперь `Option`: `None` нужен, чтобы отличить «живой worktree неизвестного владельца» от «папка вне проекта» (там хук работает по-старому).
- `main_checkout_of_worktree` не остался в `registry.rs`: его разбор переехал в `bars_core::worktree::locate` (нужен и обнаружению), старая функция стала бы мёртвым кодом.
- `resolve_cwd` держит замок реестра на время нескольких `stat` на каждом вызове без явного проекта; при замере стоит проверить цену.
- Дерево и `NestedProjectInfo` папки worktree не получают (это шаг 6.3.6); сейчас они только исключены из индексов.

## Дальше

- Шаг 6.3.2 (дельта) опирается на `Worker::linked_worktrees`, `find_worktree`, `ViewSelector` и `nested::worktree_dirs`; `session_start` и `observe_window` — места, где ставить первое обращение к виду.
- Первым делом при разрешённой сборке — `cargo fmt`, `clippy` и тесты `bars-core` (`worktree`, `nested`), `bars-worker` (`registry`, `worker::tests::worktrees`, `handler::tests::worktrees`, `project_filter`), `bars-mcp` (`render`), `bars-protocol`, `bars-cli`.
