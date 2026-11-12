---
title: Сессия 12-11-2026 — шаг 6.2.1, подпроекты в реестре и протоколе
---

# Шаг 6.2.1 — подпроекты в реестре и протоколе

Предыдущий handoff по теме: [требования и спеки worktree и подпроектов](../2026-11-12-worktree-subprojects/handoff.md). Контракт — [subprojects.md](../../features/projects/subprojects.md). Код написан без проверки сборкой: сборки, `cargo`, тесты и `check.ps1` не запускались.

## Сделано

- Реестр (`crates/bars-worker/src/worker/registry.rs`): поля `discovered` и `lazy` в `ProjectEntry` (при `false` в файл не пишутся, старый файл читается без миграции, `lazy` без `discovered` сбрасывается при загрузке с записью в журнал); `parent_of`, `main_of`, `qualified_name`, `add_discovered`, `clear_lazy`, `set_standalone`; `RegistryError::DiscoveredRename`; `resolve` принимает составное имя (`\` равен `/`, хвостовой `/` отбрасывается), имя-селектор есть только у проекта без предка и у самостоятельного подпроекта; `has_duplicate_name` считает только имена-селекторы; новая `ResolveError::UnknownSubproject` — перечень подпроектов главного (до 10) при неизвестном имени с префиксом главного.
- Протокол (`methods.rs`): `ProjectState::Lazy`; `ProjectInfo.parent`, `qualified_name`, `discovered`, `subprojects` (новый `SubprojectInfo`), `discovery_truncated`; все поля пропускаются при пустом значении.
- Воркер: `ProjectSnapshot` получил `parent`, `qualified_name`, `discovered`, `lazy`; состояние `Lazy` берётся из записи реестра; `nested_projects` отдаёт составное имя; ответ `projects` и `register` собирает `Handler::assemble_info`; ошибки: `DiscoveredRename` — `invalid_params`, `UnknownSubproject` — `project_not_found` с перечнем, прежняя `NotFound` дополнена «список — bars_status(all=true)».
- MCP и CLI: `project_arg` по новому правилу (путь — абсолютное значение, значение с точки или значение с разделителем, у которого `cwd/значение` — папка; иначе имя), один набор тестов в обоих крейтах (`project_arg_tests`); `name:` печатает составное имя; `state_name` знает `lazy`.
- Окно (только типы): `ProjectState` `"lazy"`, новые необязательные поля и `SubprojectInfo` в `types.ts`; `stateDot` для `lazy` — класс `idle`, «ждёт первого обращения»; в `theme.css` добавлен `.dot.idle` (полая серая точка). `Sidebar.svelte` и `projects.ts` не тронуты.
- Тесты реестра из спеки «Граничные случаи → Реестр и имена» в `registry.rs`: старый файл, запись и чтение полей, нормализация `lazy`, предок и главный любой глубины, имя после переименования главного, `\` и хвостовой `/`, своё имя самостоятельного и отсутствие его у найденного, два `api` в разных главных, отказ переименовать найденного, имя с `/`, совпавшее с составным, подсказка с перечнем и её предел, `clear_lazy` и `set_standalone`.
- Документы: статус и места «проект, реализации нет» поправлены в `subprojects.md`, `projects.md`, `protocol.md`, `worker.md`, `mcp-tools.md`.

## Не сделано

- Автопоиск, регистрация по селектору (`register_for_selector`), активация ленивого проекта первым обращением, пассивные вызовы — шаг 6.2.2; `discovery_truncated` пока всегда `false`.
- `filter` в `projects`, `bars_status`, CLI — шаг 6.2.3; строки `subproject_of`, `origin`, `subprojects:` в блоке `bars_status` — тоже туда: в этом шаге MCP только печатает составное имя в `name:`.
- Окно: группировка и раскрытие — шаг 6.2.4.
- Правило «`register` существующего найденного подпроекта вызывает `set_standalone`» не подключено: метод готов, вызов — в шаге 6.2.2.
- Подсказка ошибки не называет причину «папка есть, но не подпроект» (нужны правило и диск автопоиска) и в перечне подпроектов не использует `filter` (его ещё нет в инструменте, текст на него ссылается по спеке).
- Ничего не собиралось: возможны ошибки компиляции и форматирования (`cargo fmt`).

## Сложности

- `ResolveError::NotFound(String)` оставлена с одним полем: на неё опираются тесты воркера. Подсказку с перечнем несёт отдельный вариант `UnknownSubproject { name, message }`, текст которого собирается в реестре.
- Вариант `ProjectState::Lazy` у `Runtime` никто не создаёт; пока лениво только то, что человек вписал в `projects.toml`: `projects()` берёт `Lazy` из записи, а `status` по такому проекту вернёт `NotRegistered` (в `Inner.projects` его нет) — разберёт шаг 6.2.2 вместе с созданием `Runtime`.

## Дальше

- Шаг 6.2.2 опирается на `add_discovered`, `clear_lazy`, `set_standalone`, `parent_of` и на `ProjectState::Lazy`.
- Первым делом при разрешённой сборке — `cargo fmt`, `clippy` и тесты `bars-worker`, `bars-mcp`, `bars-cli`.
