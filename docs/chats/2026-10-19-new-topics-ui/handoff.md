# Передача дел: «Новые темы», экран окна (шаг 20, P4)

## Сделано
- Первая реализация открывала «Новые темы» отдельным экраном (`view = "topics"`) — автор поправил: как файл, выше всех папок, в правой панели просмотра. Переделано: `app.topicsOpen`, `openTopics()`/`closeTopics()`, панель занимает место `Viewer` в `App.svelte`.
- UI: `ui/src/lib/new-topics.ts` (типы, фильтры, подписи), `new-topics.svelte.ts` (состояние, поздние ответы, «Проверить все»), `NewTopicsPage.svelte`, `TopicConfirm.svelte`; строка в `Sidebar.svelte` выше «Корня документации», `view = "topics"` в `App.svelte`/`app.svelte.ts`; «Не хватило знаний» в `SearchPane.svelte`; `_window.request_id` у каждого поиска окна; заглушка воркера `mock.ts` (сцены `topics-empty`, `topics-error`, `topics-off`, `topics-loss`).
- Воркер: `index_token` вместо секундной метки индекса для `conclusive`; проверка получает новую ревизию темы после собственного перехода в «Есть материалы» (`crates/bars-core/src/new_topics/store.rs`, `crates/bars-worker/src/worker/timefmt.rs`).
- Документы: `features/new-topics/` (design — раздел «Экран в окне», plan, requirements, Index), `features/window/main-window.md`, `manual/new-topics.md` (новый), `manual/search.md`, `CHANGELOG.md`, PLAN.md (шаг 20 сужен до приёмки), OQ-103.
- Проверено: `npx vitest run` (26 файлов, 173 теста), `svelte-check` без ошибок; `cargo test -p bars-worker --lib` (219), `-p bars-core --lib new_topics` (22), `cargo check -p bars-app`; браузер на заглушке воркера: список, карточка, проверка, диалог удаления, ручная отметка, обе темы, узкое и широкое окно.

## Не сделано
- Сквозная приёмка A1…A14 на настоящей bge-m3 и в собранном Tauri (шаг 20 плана); release-замер записи и нагрузка при индексации; макет `design/mockup.html` не приведён к реализованному экрану.
- Клавиатура и скринридер проверены только структурой (кнопки, фокус диалога), не вживую.
- Ничего не закоммичено.

## Сложности
- Heredoc с русскими кавычками ломался в Bash-инструменте: правки документов шли скриптами Python из scratchpad.

## Дальше
- Шаг 20 плана: собрать и установить bars, пройти A1…A14, поправить макет (в нём экран из трёх колонок, а в окне — панель справа); затем закрыть IDEA-14.
