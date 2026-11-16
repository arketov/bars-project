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

- Проверка после коммита (по прямой просьбе автора 16-11-2026): `cargo fmt --check` чист; `cargo build --workspace` зелёный; `cargo test --workspace --lib --tests --bins`: `bars-worker` 495 прошли, 0 упало (в том числе три теста `like`/`similar` по worktree), `bars-core`, `bars-graph`, `bars-protocol` зелёные; установщик `build-installer.ps1` собран (`dist-installer/bars_0.1.0_x64-setup.exe`, 21,7 МБ). Не зелёное, не из этой правки: `cargo clippy` (замечания в `doc_queue.rs`, `subprojects.rs`, `bars-app/commands.rs` — 8 аргументов, тест `bars-mcp` — сложный тип); тесты `bars-mcp` `mcp` (`every_graph_tool_lists_only_its_own_parameters`, `status_filter_lists_only_matching_registry_projects`, `maintenance_tools_…`, `diagnostic_mcp_searches_…`, `pool_bad_read_…`) и `bars-cli` `register_search_outline_read_scenario` — код блока 6. Три замечания clippy (`then_some`, `is_multiple_of` в `mod.rs`, `delta.rs`, `gate.rs`) исправлены. Тесты `bars-mcp` на `like` по worktree нет.
- Промпты бота (`main.bot` в iterion) не менялись и не должны: после фикса `wt="."` им не нужен, а с ним бот потерял бы собственные правки.
- `similar` и `duplicates` по worktree остались ошибкой.
- Тест MCP на `like` по worktree не добавлен (`bars-mcp/tests`): пометка проверена только на уровне воркера.

## Починка красного после проверки (16-11-2026, по «Да, чинить»)

Полный `check.ps1 -Jobs 2`: fmt, clippy, build, `cargo test` — зелёные (bars-mcp 36 из 36, bars-worker 495). Исправлено:

- clippy: `then_some`, `is_multiple_of` (4 места), лишние заимствования в тестах `doc_queue.rs`, лишний `clone` в `subprojects.rs`, псевдоним `SeenTarget` в тесте `bars-mcp`; `document_write` и `entry_move` (8 аргументов) без `#[allow]`: `format = {eol, bom}` и `destination = {parent, name}` — структуры, `ui/src/lib/api.ts` и документы (`editor-design.md`, `link-index.md`) обновлены, у `bars-app` добавлен `serde`.
- `bars-cli`: `projects --filter` вместе с `--project` отклоняется до обращения к воркеру (`reject_filter_with_project`): clap конфликт с глобальным флагом не ловит; тест проверяет код выхода и сообщение.
- `bars-mcp`: ответ, который не разобрался в тип, снова повторяется один раз как `BadReply` (разбор перенесён внутрь `call_once`, сломано в 6.3.5); `bars_audit` с неразрешимым `project` без `wt` — ошибка в частях ответа, а не сбой вызова; описание `wt` у `bars_search` больше не говорит «like is not available»; `wt` у графовых инструментов короче, предел схем шести инструментов 15 КБ вместо 14 (`graphrag/tools.md`).
- Тесты `bars-mcp`: `status_filter…` регистрировал проект до старта воркера; `diagnostic_mcp_searches…` и `agent_answers_are_text` ловили проверку, которую ставит первый вызов MCP к проекту (Т-8): добавлен первый вызов и ожидание.
- `ui`: `SettingsPage.svelte` держал два старых порога (`min_score_rerank`, `min_score_vector`), тип `SearchDoc` знает один `min_score` (ADR-0007): лишние поля убраны, `svelte-check` зелёный; тест чипов `SearchPane` игнорирует класс Svelte.

Четыре красных теста vitest оказались устаревшими ожиданиями, не ошибками кода (код окна менялся шагом 2.4.2 от 05-11-2026): в `new-topics-search.test.ts` убраны `mode: 'hybrid'` (режимов поиска нет) и тест диагностики пустой выдачи (`diagnoseEmpty` убрана намеренно, дизайн `global-search-design.md`); в `search-results.test.ts` образец текста не содержит «возвращаются» (основа «возвр» по документированному правилу его захватывает); в `worktree-view.test.ts` селектор галочек основного checkout отсекает галочки внутри папок worktree. Итог: `check.ps1 -Jobs 2` зелёный целиком (fmt, clippy, build, cargo test, svelte-check, vitest 771 из 771).

## Сложности

- `Scope::build_with_paths`, `IndexView::cosines` и `load_hits` были видны только внутри `hybrid`; пришлось расширить видимость до `crate::search`, не открывая наружу.
- В `bars-project` уже была незакоммиченная правка `docs/manual/iterion.md` — не моя, в коммит не вошла.

## Дальше

- При разрешённой сборке: `cargo fmt`, `clippy`, тесты `bars-core` (`search`), `bars-worker` (`handler::tests::worktree_search`), `bars-mcp` (`mcp`).
- Если агентам понадобится `similar` по worktree — тот же `similar_in_view` уже готов, нужен только снять ранний `return` в `WorkerHandler::similar` и разрешить чанк со смещением.
