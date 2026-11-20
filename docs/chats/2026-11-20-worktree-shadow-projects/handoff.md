---
title: Сессия 20-11-2026 — папка внутри worktree не регистрируется проектом, прежние записи скрыты
---

# Папка внутри worktree: «не распознаёт worktree»

Контракт обнаружения и вида worktree — [worktrees.md](../../features/projects/worktrees.md); предыдущий handoff по теме — [like по worktree](../2026-11-16-like-in-worktree/handoff.md). Код написан без проверки сборкой: сборки, `cargo`, тесты и `check.ps1` не запускались.

## Поводом было

Скриншот окна: под проектом `iterion` пять строк вида `.worktree/feat-X/docs/features/X`, у одной «индексация · ждёт». Замечание пользователя: «Не распознаёт worktree».

## Что выяснилось

- Обнаружение worktree работает: `bars_status` по `iterion` показывает 105 worktree, `resolve_cwd` для `cwd` в `W/docs/features/X` даёт владельца `iterion` с видом.
- Пять строк — самостоятельные записи `projects.toml` (регистрация 16–20-11-2026, в каждой папке свой `.bars/config.toml` от момента регистрации). Их добавили через «Добавить папку» в окне: `Worker::register` не проверял, что папка лежит внутри живого worktree.
- `repository_of` у такой записи возвращает `None` (выше по пути файл `.git`), поэтому владельца у неё нет; `Registry::parent_of` группирует по одному пути и сажал её под `iterion` подпроектом. Итог: второй индекс тех же документов и строка `.worktree/…` в списке.

## Сделано

- `Registry::worktree_shadow` / `is_shadow` / `ensure_registrable`: папка внутри живого worktree зарегистрированного проекта (владелец — не она сама) — тень вида. `Registry::add` и `Worker::register` отказывают (`RegistryError::InsideWorktree`, ответ `invalid_params`); в `Worker::register` проверка стоит до `prepare_project`, чтобы `.bars/config.toml` не создавался.
- Прежние записи остаются в `projects.toml`, но `Worker::projects` их не отдаёт (окно, `bars_status`, `projects`), а `Inner::activate` их не проверяет и не индексирует. Исчез worktree — запись снова обычный проект.
- Тесты в `registry.rs`: `folder_inside_a_live_worktree_is_a_shadow_and_is_not_registered`, `entry_added_before_the_check_is_hidden_while_the_worktree_lives` (не запускались).
- Документы: `worktrees.md` (раздел «Папка внутри worktree не регистрируется проектом», «Окно»), `projects.md`, `subprojects.md`.

## Не сделано

- Сборка и тесты не запускались (запрет автора на сборки); текст ошибки и скрытие в окне не проверены на живом окне.
- Диалог «Добавить папку» заранее не предупреждает: ошибка приходит при нажатии «Добавить». Предупреждение в `project_root_suggest` (`inside`) — отдельный шаг окна.
- Тесты воркера на `register` внутри worktree нет: проверка покрыта на уровне реестра.
- Файлы `.bars/` пяти папок на диске не трогались; индекс-файлы остаются. Скрытую запись из окна не убрать (её нет в списке); убирается `bars-cli unregister <путь>` или правкой `~/.bars/projects.toml` (команда `unregister` в `bars-cli` есть; как она разбирает путь скрытой записи, не проверял). `rustfmt --check` по трём изменённым `.rs` чист.
- Установленные `bars.exe`, `bars-cli.exe`, `bars-mcp.exe` от 12-11-2026: исправление заработает после новой сборки и установки.

## Дальше

- Предупреждение в диалоге добавления папки и тест воркера на отказ регистрации.
- По слову автора — проверка сборкой: `cargo test -p bars-worker registry` и один проход окна на `iterion`.
