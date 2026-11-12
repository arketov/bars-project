---
title: Сессия 12-11-2026 — шаг 6.3.5, wt и --wt в MCP и CLI
---

# Шаг 6.3.5 — `wt` в MCP, `--wt` в CLI, пометки «граф и аудиты — у основного»

Предыдущий handoff по теме: [актуальность дельты worktree](../2026-11-12-6-3-4-worktree-freshness/handoff.md) (там же ссылки на поиск 6.3.3, дельту 6.3.2 и обнаружение 6.3.1). Контракт — [worktrees.md](../../features/projects/worktrees.md), пункт 5 «Порядка реализации» и раздел «Как это устроено в MCP и CLI (шаг 6.3.5)». Код написан без проверки сборкой: сборки, `cargo`, тесты и `check.ps1` не запускались; сигнатуры и вызывающие проверены чтением.

## Сделано

- Протокол (`bars-protocol/methods.rs`): `wt_value`, `owner_only_note`, `view_worktree`; `SessionInfoParams.project/wt`, `SessionProjectInfo.worktree`, `SessionWorktreeInfo`.
- MCP: `wt` у `bars_search` (обе формы), `bars_read`, `bars_status`, `bars_audit` и шести графовых инструментов; английские описания (НФТ-4); `target(project, wt)` передаёт `wt` как есть (`"."` не превращается в путь). `call_noted` читает `worktree_note` из ответа: строка над текстом у графа, `view=links`, JSON графа — поле. Аудиты и «Новые темы» (у них нет `Target`) получают пометку и проверку `wt` через `session_info` с `project`/`wt` (`audit_worktree_note`). Шапка поиска — `worktree <имя> (<ветка>, delta <состояние>)`, `note:` из `applied.scope.worktree.note` под шапкой. `bars_status` с `wt` (без `all`/`filter`), `refresh=true`+`wt` — сверка вида, пометка перед AUDITS. `initialize` называет проект «Project X, worktree `имя` (branch …)». Списки разрешённых полей истории (`observation.rs`, `mcp_history.rs`) знают `wt`.
- Воркер: `reindex` с видом worktree — `Worker::resync_worktree` (новый, `delta.rs`); `full`/`accept_model_change` — `invalid_params` с `data.kind = worktree_unsupported`; владелец не готов — `project_unavailable`. `observed` зовёт `observe_worktree_mcp` на каждом вызове MCP с видом worktree (явным или из `cwd`). `session_info` разрешает цель через `resolve_view` и отдаёт `worktree`.
- CLI: глобальный `--wt`; `register`, `unregister`, `projects`, `install`, `uninstall`, `autostart` с ним отклоняются `invalid_params` до обращения к воркеру; `projects --filter` несовместим на уровне clap; `worktree_note` печатается над текстом, `--json` как есть; `reindex --wt` — «Сверка worktree принята»; поиск печатает `worktree` в «Область» и пометку над результатами.
- Тесты (не запускались): `bars-mcp/tests/support/worktree.rs` (`wt_parameter_reaches_target`, `audit_with_wt_has_owner_only_note_and_unknown_wt_is_an_error`, `wt_parameter_is_described_in_english_for_every_tool`), `bars-cli/tests/cli.rs` (`wt_parameter_reaches_target` — WORKTREES с `--wt`, `worktree_not_found` с кодом 1, `reindex --wt`, `reindex --full --wt`, `register --wt`; `wt_flag_is_checked_before_the_worker_is_called`), `handler/tests/worktree_mcp.rs` (`reindex_with_wt_resyncs_the_delta_and_full_is_rejected`, `status_with_wt_lists_only_that_worktree_without_building_it`, `session_info_names_the_worktree_of_the_session`), `instructions.rs` (`session_in_a_worktree_names_it_after_the_project`). Существующий `every_graph_tool_lists_only_its_own_parameters` дополнен общим полем `wt`.
- Документы: `worktrees.md`, `mcp-tools.md`, `agent-layer-project.md`, `cli.md`, `protocol.md`, руководство (`projects.md`, `agents.md`), `chats/Index.md`.

## Не сделано

- Ничего не собиралось и не запускалось: возможны ошибки компиляции, `clippy`, `cargo fmt`.
- `BARS_INSTRUCTION.md` и skills параметры инструментов не перечисляют (проверено поиском по bars) — не менялись.
- Пометка у графового текста `mix` по `applied.scope.worktree.note` внутри `render_graph_search_text` не выводится: видна только `worktree_note` воркера.
- `bars_status` пассивный (`wt` без `refresh`) дельту не строит, но продлевает аренду слежения — по спецификации «каждый вызов MCP с `wt`».
- Автоматической проверки `like` по worktree в MCP нет: ошибку `worktree_unsupported` показывает `failure()` как есть (тест воркера из 6.3.3).

## Сложности

- Параллельная сессия 6.3.6 правит `handler/mod.rs` (дерево) и `worktree_view.rs`. Моя правка `mod.rs` — один блок `reindex`; `worktree_view.rs` не менялся (пометка продублирована как `bars_protocol::owner_only_note`). В коммит `mod.rs` попал только блок `reindex` через `git apply --cached`; остальное осталось в рабочей папке за 6.3.6.
- У аудитов и «Новых тем» нет `Target`, а менять десять структур протокола вслепую рискованно: поэтому проверка `wt` идёт отдельным вызовом `session_info` (он не пишется в историю MCP, в отличие от `status`).
- `call_once` теперь возвращает `Value` (через `call_value`), разбор в тип — в `call_noted`; ошибка разбора ответа больше не повторяется прокси один раз.
- В `handler/tests/worktree_search.rs` вспомогательные функции сделаны `pub(super)` для нового файла тестов.

## Дальше

- Первым делом при разрешённой сборке: `cargo fmt`, `clippy`, тесты `bars-protocol`, `bars-mcp` (`mcp`, `instructions`), `bars-cli` (`cli`), `bars-worker` (`handler::tests::worktree_mcp`, `worktree_search`).
- 6.3.6 (окно): `observe_worktree_window`, `edit_worktree_files`; 6.3.7 — бенч сопоставимости `f`.
