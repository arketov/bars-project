---
title: Сессия 12-11-2026 — шаг 6.3.2, дельта worktree
---

# Шаг 6.3.2 — дельта worktree: хранилище, сверка, ленивое построение, уборка

Предыдущий handoff по теме: [обнаружение worktree и разрешение цели](../2026-11-12-6-3-1-worktree-discovery/handoff.md). Контракт — [worktrees.md](../../features/projects/worktrees.md), пункт 2 «Порядка реализации». Код написан без проверки сборкой: сборки, `cargo`, тесты и `check.ps1` не запускались; сигнатуры, вызывающие и порядок замков проверены чтением.

## Сделано

- `bars-core::indexing::worktree_delta` (новый): `open_delta` (файл `<владелец>/.bars/worktrees/<имя>/index.sqlite` через `Index::open` + таблица `wt_files` + `delta_version`, `worktree_name`), `check_delta` (отпечаток модели, `chunking_version`, нарезка, `docs_root`, формат; `Fresh`/`Compatible`/`Mismatch`), `OwnerSnapshot` с подписью `owner_digest`, трейт `OwnerView` (замок владельца — дело воркера; `IndexOwner` — прямая реализация) и `sync` с областью «все документы» или список путей: обход `walk_docs_excluding` по корню вида, пропуск по размеру и времени, хеш через общую `indexer::content_hash` (ей же пользуется `process_file`), сравнение с владельцем, запись отличающихся документов по одному транзакцией, векторы дельта → владелец → модель, уборка осиротевших векторов, повтор прохода при смене подписи владельца (не больше трёх), метки `owner_digest`, `synced_at`, `ready`. Нечитаемый UTF-8 удаляется из вида, занятый файл остаётся прежним.
- `indexer.rs`: `content_hash` (pub), `in_update_scope`, `embed_with_retries`, `check_stored_dim`, `check_vectors`, `modified_ns`, `IndexSettings::layout` стали `pub(crate)`; поведение индексатора не менялось.
- Воркер, `worker/delta.rs` (новый): состояния `none`/`building`/`ready`/`error` в памяти (`ready` восстанавливается из файла проверкой), очередь сверок с ключом `(владелец, имя)` (слияние, приоритеты), задание `delta_job` (снимок владельца под замком проекта на чтение, сверка метаданных, удаление несовместимой дельты, `sync`), `check_deltas` — проверка старта и часовая (пропавшие и оторванные worktree, смена модели, нарезки, `docs_root`, новый формат → удаление; `ready` из файла; отставшая или недостроенная дельта → `Periodic`; владельцу в `reindex_required`/`unavailable`/ленивому заданий нет), `Worker::ensure_worktree_delta`, `resync_worktree_paths` (шов для 6.3.4), `worktree_delta_status`.
- Подключения: очередь дельт стартует после активации проектов, часовой таймер зовёт `check_deltas` после обнаружения worktree; `is_idle` учитывает дельты; очередь проектов ждёт, пока идёт сверка (`delta_busy`); первое обращение — хук `session_start` (сессия в W) и поиск из worktree (`search_with_graph` в обработчике); `status` отдаёт настоящее состояние дельты; снятие и полное удаление проекта отменяют задания дельт, удаление ждёт закрытия файла и убирает `.bars/worktrees/` (проверка перенаправленных папок как у `.bars`).
- Тесты (не запускались): `bars-core` — `worktree_delta/tests.rs` (`delta_of_identical_worktree_has_no_documents_and_no_embed`, `one_section_edit_embeds_only_that_section`, `delta_hash_matches_indexer_hash`, `owner_swap_triggers_recheck_without_embed`, правка/новый/удалённый/возврат, `check_delta_…`); воркер — `worker/tests/delta.rs` (`first_session_in_worktree_builds_delta_without_embedding`, правка пути, `removed_worktree_…`, `detached_worktree_…`, `model_change_deletes_deltas`, `chunking_or_docs_root_change_deletes_deltas`, `hourly_check_restores_ready_…`, `project_deletion_removes_the_delta_folder`); обработчик — `search_from_a_worktree_requests_the_delta`. `worktree_discovery_creates_no_delta` остался от 6.3.1.
- Документы: `worktrees.md` (статус, «Как это устроено в воркере», очередь и конкурентность, поколение владельца, таблица проверок), `worker.md`, `schema.md`, `files.md`, `deletion.md`, `manual/projects.md`.

## Не сделано

- Ничего не собиралось и не запускалось: возможны ошибки компиляции, `clippy` и `cargo fmt`.
- Поиск и чтение по виду (`IndexView`, слияние пула), слежение за W, публикация путей завершённых заданий владельца, `owner_digest` после перезапуска как триггер по путям — шаги 6.3.3–6.3.4 (сейчас отставший владелец ловится только проверкой старта и часовой, полной сверкой).
- Триггеры «открытие worktree в окне» и `bars_status` с `refresh` и `wt` (параметра `refresh` с `wt` в протоколе пока нет): они вызовут `Worker::ensure_worktree_delta` в шагах 6.3.5–6.3.6.
- Замеры времени первой сверки (НТР-1) и часовой проверки (НТР-3) — нужна сборка.
- Тест `first_search_with_wt_builds_delta` по параметру `wt`: поиск по виду появится в 6.3.3; сейчас проверен путь «поиск из cwd в worktree».

## Сложности

- Спека говорит «задание той же очереди». `JobQueue` ключуется путём проекта и связана с журналом операций, поэтому сделана отдельная очередь дельт; правило «одно задание индексации за раз» сохранено флагами (`delta_busy`, очередь проектов пуста и не занята). Следствие: долгая сверка откладывает задания проектов. Спека поправлена как «выбор».
- «Поколение индекса владельца» у воркера нет (есть поколение регистрации); вместо него — подпись `owner_digest` по `(path, hash)`, перечитываемая в конце прохода. Цена — один лишний проход по `documents` владельца на сверку.
- Порядок замков: `is_idle` держит очередь проектов, потом дельты; задание дельты берёт замок очереди только после отпускания замка дельт.
- Каждое обращение из поиска не должно ставить полную сверку: `ensure_worktree_delta` ничего не ставит, если запись уже есть и не в ошибке.
- Проверка старта запускается после активации проектов, иначе состояния владельцев ещё не созданы и «готова» не восстанавливается.
- В `check_deltas` кратковременный сбой обнаружения (пустой список worktree) удалит дельты: они производные, цена — повторное построение.

## Дальше

- Шаг 6.3.3 опирается на `worktree_delta::view_files` (`wt_files`), `is_ready`, `delta_path`, `DeltaState` из `Worker::worktree_delta_status` («строится» — пометка в ответе); исключённые пути считаются из `wt_files`, `documents` дельты и `documents` владельца.
- Шаг 6.3.4: `Worker::resync_worktree_paths`, подпись `owner_digest` в `meta`; публикация путей завершённых заданий владельца — в `JobQueue`.
- Первым делом при разрешённой сборке — `cargo fmt`, `clippy` и тесты `bars-core` (`indexing::worktree_delta`, `indexer`), `bars-worker` (`worker::tests::delta`, `handler::tests::worktrees`, `worker::tests::deletion`).
