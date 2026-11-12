---
title: Сессия 12-11-2026 — шаг 6.3.3, поиск и чтение по worktree
---

# Шаг 6.3.3 — поиск и чтение по виду worktree

Предыдущий handoff по теме: [дельта worktree](../2026-11-12-6-3-2-worktree-delta/handoff.md) (там же ссылка на обнаружение, шаг 6.3.1). Контракт — [worktrees.md](../../features/projects/worktrees.md), пункт 3 «Порядка реализации» и раздел «Как устроен поиск по виду в коде». Код написан без проверки сборкой: сборки, `cargo`, тесты и `check.ps1` не запускались; сигнатуры и вызывающие проверены чтением и `grep`.

## Сделано

- `bars-core`, `search/hybrid/view.rs` (новый): `DELTA_ID_OFFSET`, `IndexView` (`single`, `worktree` с `temp.wt_excluded`, `source`, `text_scored`, `cosines`, `f_signal` по базе, `p_signal`, `retain`, `load_hits`, `chunk_document`, `section_span`, `count_candidate_modules`, `chunk_count`), трейт `Catalog` для `Index` и `IndexView` (область по объединению документов вида).
- `hybrid.rs`: все читатели номеров чанков идут через `IndexView`; `search_in_view` — поиск по виду; `search`, `search_with_signals`, `search_with_links`, `scored_pool`, `candidate_trace` — оболочки с одним источником (поведение прежнее, тесты `hybrid/tests.rs` не правились); `Scope::build_with_paths` принимает каталог, `Scope::excluding_wt`; с двумя источниками расширение связями и `l` отключены.
- `worktree_delta::excluded_paths` → `ExcludedPaths { paths, pending, delta_documents }`.
- Протокол: `SearchScope.worktree: Option<SearchWorktree>` (`name`, `branch`, `state`, `delta_documents`, `pending`, `note`).
- Воркер, `handler/worktree_view.rs` (новый): кэш исключённых путей по подписи файлов баз, `plan_worktree_search` (готова — слияние; строится, нет, ошибка, дельта не открылась — владелец с пометкой), `usable_view`, `view_root`, `disk_documents`, `worktree_unsupported`, `with_note`. `search_with_graph` ищет по виду, не пополняет «Новые темы» и не грузит граф при слиянии; `read`, `outline`, `disk_text`, папка работают по диску W без дельты; `like`, `similar`, `duplicates` с worktree — `invalid_params`/`worktree_unsupported`; `dispatch` дописывает `worktree_note` в ответы графа и `disk_links` по worktree.
- Тесты (не запускались): `bars-core` — `search/hybrid/view/tests.rs` (`document_created_only_in_worktree_is_found_only_with_wt`, `document_deleted_in_worktree_is_not_found_with_wt`, `worktree_without_edits_gives_owner_results_bitwise`, `unrelated_edit_does_not_change_scores_of_other_documents`, `merged_pool_has_no_chunk_id_collision`, `in_modules_files_resolve_over_union_of_owner_and_delta`, `excluded_paths_cover_deleted_and_replaced_owner_documents`); воркер — `handler/tests/worktree_search.rs` (`first_search_with_wt_builds_delta`, `search_while_building_answers_from_owner_with_note`, `read_outline_occurrences_use_worktree_disk_without_delta`, `graph_audit_links_answer_from_owner_with_note`, `like_with_worktree_is_unsupported`).
- Документы: `worktrees.md`, `hybrid-search.md`, `protocol.md`, `chats/Index.md`.

## Не сделано

- Ничего не собиралось и не запускалось: возможны ошибки компиляции, `clippy` и `cargo fmt` (в `handler/mod.rs` есть строки длиннее 100 знаков).
- `wt` у аудитов, `links_of` и `links_into`: они принимают `project`, а не `Target` — пометка и `wt` для них появятся с MCP и CLI (шаг 6.3.5); ошибка «документа нет у владельца» с пометкой для ссылок не сделана.
- Текстовая выдача (`bars-mcp`, `bars-cli`): `applied.scope.worktree.note` и `worktree_note` попадают в JSON, но не нарисованы строкой над результатами — шаг 6.3.5.
- Бенч поиска на worktree с правкой (сопоставимость `f`, шаг 6.3.7) и замеры времени — нужна сборка.

## Сложности

- Кэш «поколение владельца, поколение дельты» из спецификации: поколений у воркера нет (6.3.2 тоже ввёл подпись `owner_digest`), поэтому кэш исключённых путей ключуется подписью `index.sqlite` и `-wal` обеих баз (длина, время изменения); цена — пересчёт списка при любом коммите.
- Номера документов в слое тоже со смещением (`document_id` у свёртки раздела и `section_span`): иначе документы двух баз с равными локальными номерами слились бы в один раздел.
- `like`, `similar`, `duplicates` работают по `Index` и `Scope::build`; их не трогали, а в обработчике закрыли ошибкой.
- Файлы `worker/delta.rs`, `doc_queue.rs`, слежение (6.3.4) не менялись.

## Дальше

- Первым делом при разрешённой сборке: `cargo fmt`, `clippy`, тесты `bars-core` (`search::hybrid`, `indexing::worktree_delta`) и `bars-worker` (`handler::tests::worktree_search`, `worktrees`); затем бенч сопоставимости `f` (6.3.7).
- Шаг 6.3.5: показать `note` и `worktree_note` строкой над результатами, `wt` в схемах MCP и флагах CLI, пометка у аудитов.
