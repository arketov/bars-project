---
title: Сессия 12-11-2026 — шаг 6.3.6, worktree в окне
---

# Шаг 6.3.6 — окно: папка «worktree», изоляция выбора, редактор в worktree

Предыдущий handoff по теме: [актуальность дельты (6.3.4)](../2026-11-12-6-3-4-worktree-freshness/handoff.md); там же и в [6.3.2](../2026-11-12-6-3-2-worktree-delta/handoff.md), [6.3.1](../2026-11-12-6-3-1-worktree-discovery/handoff.md), [6.2.4](../2026-11-12-6-2-4-project-switcher/handoff.md) лежат точки входа, которыми пользуется этот шаг. Контракт — [worktrees.md](../../features/projects/worktrees.md), раздел «Окно» и «Как устроено в окне (шаг 6.3.6)». Код написан без проверки сборкой: `cargo`, `npm`, `svelte-check`, тесты и `check.ps1` не запускались.

## Сделано

- **Макет.** `docs/features/window/mockups/worktree-view.html` — четыре сцены в обеих темах (папка «worktree», раскрытый вид с галочками и подсказкой, вкладка и плашка результата, неактивные граф/аудиты/«Новые темы»). Первая версия открылась без стилей (файл смотрели отдельно от `theme.css`) — тема встроена в файл; автор согласовал с правкой «текст и тег без отступа» (отступ плашки у заголовка карточки исправлен).
- **Воркер.** `handler.tree` с `wt`: документы владельца, поверх них документы готовой дельты (`worktree_view::tree_documents`), первое обращение ставит дельту.
- **bars-app.** `commands::view_roots`/`ViewRoots` (корень вида, владелец, имя), `wt` у `document_read`/`document_write`/`document_revision`/`document_action`/`tree_read`/`entry_*`; запись в виде зовёт `Worker::edit_worktree_files`; `tree_read` с `wt` ставит дельту и аренду окна; новая команда `worktree_observe` (зарегистрирована в `main.rs`); `tree_nested_json` добавляет в `nested` папки worktree с `kind: "worktree"` (и в снимке окна); `tree_io::plan_move_without_links`/`move_without_links` — перенос без обновления ссылок, план отдаёт `links_updated: false`; `entry_plan_delete` в виде ссылок не считает; дерево вида не попадает в кэш папок.
- **Окно.** `view-key.ts` (ключ вида `проект␀wt:имя` — «проект» буферов и вкладок), `api.ts` (команды разбирают ключ), `types.ts` (`WorktreeStatus`, `SearchWorktree`), `WorktreeFolder.svelte` (папка, ветка и состояние дельты, неактивные граф/аудиты/«Новые темы»), `TreeRow` с `wt` и серой пометкой «worktree», `app.svelte.ts` (`worktrees`, `wtTrees`, `selectionView`/`selectionHint`, `openWorktree`, `focusWt` и аренда раз в 10 с, поиск с `wt`, `openDocument` с `wt`), плашка и пометка выдачи (`ResultCard`, `SearchPane`), вкладка документа worktree (`Workspace`, `WorkspaceDocument`, `workspace-layout`).
- **Тесты (не запускались).** `ui/src/lib/worktree-view.test.ts` — `worktree_folder_in_tree_and_isolated_selection` (+ ключ вида, исчезнувший worktree); `crates/bars-app/src/worktree_view_tests.rs` — запись в файл W, допуск путей, перенос без ссылок, устаревший токен.
- **Документы.** `worktrees.md` (статус, «Как устроено в окне», таблица проверок, порядок), `main-window.md`, `tree-and-files.md`, `subprojects.md`, `manual/projects.md`, `mockups/Index.md`.

## Не сделано

- Ничего не собиралось и не запускалось: возможны ошибки компиляции, `clippy`, `svelte-check`, падения тестов; проверка в окне вручную не делалась.
- Пунктов контекстного меню для строк и вкладок в виде worktree нет (у них снят `data-menu-kind`): операции дерева (`entry_*`) работают на стороне команд, но из окна их пока не вызвать; диалог переноса не показывает `links_updated: false`.
- Отдельное окно документа worktree, ссылки и обратные ссылки внутри вкладки worktree, `folder_target` (открыть папку worktree) — нет.
- Строки worktree в переключателе проектов не сделаны (решение шага: папка дерева); `subprojects.md` поправлен.
- Сводка «Пример `examples/*_probe.rs`» не обновлялась: они перечисляют команды выборочно, `worktree_observe` там не нужна.

## Сложности

- Документы worktree живут как «проект с ключом вида» в буферах редактора и параметрах вкладки (`project` вкладки — ключ). Это дало буферы и закрытие вкладок без правок, но запись о вкладке в сохранённой раскладке несёт ключ: `decodeLayout` принимает ключ вида владельца этого проекта и id с суффиксом `:wt:имя`. Принудительная запись несохранённых правок перед переносом (`flush::request_clean`) смотрит на проект-владелец и буферов worktree не видит.
- Тесты окна подменяют `./api`; ключ вида вынесен в отдельный чистый `view-key.ts`, иначе подмены без `viewKey` ломали бы существующие тесты.
- `handler/mod.rs` правился параллельной сессией 6.3.5 в том же checkout; мой хунк — только метод `tree`.
- Выбор `wt` для результатов поиска окна: поиск без выбора в worktree идёт по основному checkout (параметр `wt` не передаётся), чтобы не менять форму вызова существующих тестов.

## Дальше

- По слову автора: `cargo fmt`, `clippy`, тесты `bars-app` (`worktree_view_tests`, `link_move_tests`, `tree_io_tests`), `bars-worker` (`handler::tests`), `npm run test` (`worktree-view.test.ts` и соседи, которые подменяют `./api`), `svelte-check`, ручная проверка из `worktrees.md` («Ручная проверка»).
- Шаг меню: пункты «Создать», «Переименовать», «Переместить», «В корзину» для строк worktree, текст «ссылки не обновляются» в диалоге переноса.
