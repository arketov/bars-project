---
title: Handoff 2026-11-03 — исключение вложенных проектов из внешнего (шаг 3.1.6)
---

# Handoff: исключение вложенных проектов

Продолжение темы: [проект вложенных проектов](../2026-11-03-nested-projects-design/handoff.md).

## Сделано

- Код по принятому проекту (OQ-255…OQ-257): внешний проект исключает индексируемую область вложенного из обхода, индекса, индекса ссылок, аудитов, дерева, слежения и графа (граф — через удаление документов из индекса: частичное обновление 1.1.4 убирает их без полной перестройки). Чтение и `bars_text_search` остались по диску.
- Воркер считает области по реестру и `docs_root` каждого проекта, публикует их при старте, регистрации, снятии, удалении и смене `docs_root` и ставит внешнему явную проверку при изменении списка. Пропавший вложенный остаётся исключённым, пока есть запись.
- Окно: пометка «вложенный проект» без галочки и с «Открыть проект» в дереве; в диалоге добавления — строки «её документы (N файлов) уйдут из индекса A» и «найдены вложенные проекты, они исключены».
- Агент: строка «исключены вложенные проекты: B (ищите project=B)» в `bars_projects` и в `scope` поиска (MCP).
- Тесты написаны и не запускались: core (`nested`, `walk`), `bars-graph` (ссылка внутрь области не битая), `bars-protocol` (строка агенту), worker (приёмка Т-7: уход области, возврат после снятия, пропавший остаётся исключённым), UI (`tree.test.ts`).
- Документы: [projects.md](../../features/projects/projects.md) (статус, устройство в коде), [updates.md](../../features/indexing/updates.md), [worker.md](../../architecture/worker.md), [link-index.md](../../features/knowledge-base/link-index.md), [tree-and-files.md](../../features/knowledge-base/tree-and-files.md), [mcp-tools.md](../../features/agent-tools/mcp-tools.md), [module-scope.md](../../features/agent-tools/module-scope.md), [руководство по проектам](../../manual/projects.md).

## Не сделано

- Ничего не собиралось, тесты и общая проверка не запускались (запрет автора): код мог не скомпилироваться.
- Приёмка Т-15 тестом обработчика (вызов без проекта из `B/docs` и из `A` вне `B`) не написана: проверена только строка агенту в протоколе и worker-тест списка `nested_projects`.
- `bars-cli projects` строку об исключённых проектах не печатает.
- Вики-ссылка на имя файла из исключённой области остаётся битой.
- Нет проверки на `tmc-opovechenie` и `overdrive`.

## Сложности

- `IndexSettings` создаётся в десятках мест, поэтому список областей не передаётся полем настроек, а публикуется в процессе (`bars_core::indexing::nested`); окно встроено в процесс воркера и читает тот же список. `publish` обновляет записи переданных проектов и не трогает чужие.
- `ProjectInfo` и `SearchScope` получили поля; литералы в тестах других крейтов (`bars-cli`, `bars-mcp`) могли остаться без них — сборка покажет.

## Файлы кода

`crates/bars-core/src/indexing/{nested.rs (новый),mod.rs,walk.rs,indexer.rs}`, `crates/bars-core/src/link_index/store.rs`, `.../store/page.rs`, `crates/bars-core/src/audits/mod.rs`, `crates/bars-graph/src/links.rs`, `.../links/tests.rs`, `crates/bars-protocol/src/{methods.rs,search_scope.rs,lib.rs}`, `crates/bars-worker/src/worker/{mod.rs,deletion.rs,tests.rs}`, `crates/bars-worker/src/handler/{mod.rs,graph.rs}`, `crates/bars-mcp/src/render.rs`, `crates/bars-app/src/{commands.rs,window_updates.rs,tree_io.rs,folders.rs}`, `ui/src/lib/{api.ts,app.svelte.ts,tree.ts,tree.test.ts,TreeRow.svelte,ProjectRootDialog.svelte}`.

## Дальше

Собрать и прогнать `scripts/check.ps1` по разрешению автора, починить ошибки компиляции, написать тест Т-15 обработчика, по желанию — строка в `bars-cli projects`.
