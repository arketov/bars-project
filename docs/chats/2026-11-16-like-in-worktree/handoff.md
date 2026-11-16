---
title: Сессия 16-11-2026 — like по worktree работает по виду, а не ошибкой worktree_unsupported
---

# `bars_search like` по worktree

Предыдущий handoff по теме: [поиск и чтение по worktree, шаг 6.3.3](../2026-11-12-6-3-3-worktree-search/handoff.md) (там `like`, `similar` и `duplicates` закрыли ошибкой) и [wt в MCP и CLI, шаг 6.3.5](../2026-11-12-6-3-5-worktree-mcp-cli/handoff.md). Контракт — [worktrees.md](../../features/projects/worktrees.md), раздел «Чего в worktree нет» и пункт «`like` по виду» в «Как устроен поиск по виду в коде». Код написан без проверки сборкой: сборки, `cargo`, тесты и `check.ps1` не запускались.

## Поводом было

Замечание пользователя: `bars_search like` падает с `worktree_unsupported` на worktree проекта `iterion`; `like` стоит в промптах бота (`spec`, `spec-piece`, `spec-accept` в `main.bot`), а workspace бота сам является worktree, поэтому `wt` подставляется из `cwd` и без `wt="."` вызов всегда падает. `bars_read view=links` при этом отвечает по основному checkout с пометкой.

## Сделано

- Причина: запрет `like` по worktree вывел агент в 6.3.3 из «граф, аудиты и ссылки — только у основного checkout»; автор в OQ-314 назвал только поиск, чтение и дерево. Решение пользователя этой сессии — вариант B: `like` работает по виду, `similar` и `duplicates` остаются с ошибкой (в `bars_search` их нет).
- Ядро (`bars-core/src/search`): `similar_in_view` и `chunk_public_at_line_in_view` — `similar` и `chunk_public_at_line` стали оболочками над ними с `IndexView::single` (поведение вида из одного источника то же). Источник `путь:строки` ищется в каталоге вида: документ дельты читается из дельты, ID чанка получает смещение `DELTA_ID_OFFSET`; косинусы — `IndexView::cosines` по обеим базам; документ источника исключён из области. `Catalog` реэкспортирован внутри `search`, `IndexView::cosines` и `load_hits` стали `pub(in crate::search)`.
- Воркер (`worktree_view.rs`): `WorkerHandler::like_in_worktree` — тот же выбор вида, что у поиска (готовая дельта и исключённые пути, иначе владелец с пометкой «индекс строится / не построен / ошибка»); `linked_from` — по индексу ссылок владельца; ошибка документа worktree при ответе по владельцу несёт пометку. `like` в `mod.rs` отправляет сюда вид worktree.
- Протокол: `LikeOutput.worktree_note` (необязательное). MCP: `bars_search like` читает пометку через `call_noted` и ставит строкой над ответом.
- Тесты (не запускались): в `handler/tests/worktree_search.rs` `like_with_worktree_is_unsupported` заменён на `similar_duplicates_with_worktree_are_unsupported`, добавлены `like_in_worktree_finds_source_in_delta_and_marks_links` и `like_while_delta_builds_answers_from_owner_with_note`.
- Документы: `worktrees.md`, `protocol.md`, `agent-layer-project.md`, руководство `manual/projects.md`, `chats/Index.md`.
- `rustfmt --check` по изменённым файлам чист; диагностика rust-analyzer через serena по ним пустая (это не сборка).

## Не сделано

- Ничего не собиралось и не запускалось: возможны ошибки компиляции, `clippy`, падения новых тестов (особенно `like_while_delta_builds…`: поведение паузы очереди скопировано с `search_while_building…`).
- Промпты бота (`main.bot` в iterion) не менялись и не должны: после фикса `wt="."` им не нужен, а с ним бот потерял бы собственные правки.
- `similar` и `duplicates` по worktree остались ошибкой.
- Тест MCP на `like` по worktree не добавлен (`bars-mcp/tests`): пометка проверена только на уровне воркера.

## Сложности

- `Scope::build_with_paths`, `IndexView::cosines` и `load_hits` были видны только внутри `hybrid`; пришлось расширить видимость до `crate::search`, не открывая наружу.
- В `bars-project` уже была незакоммиченная правка `docs/manual/iterion.md` — не моя, в коммит не вошла.

## Дальше

- При разрешённой сборке: `cargo fmt`, `clippy`, тесты `bars-core` (`search`), `bars-worker` (`handler::tests::worktree_search`), `bars-mcp` (`mcp`).
- Если агентам понадобится `similar` по worktree — тот же `similar_in_view` уже готов, нужен только снять ранний `return` в `WorkerHandler::similar` и разрешить чанк со смещением.
