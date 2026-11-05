---
title: Handoff — выдача окна по макету, шаг 2.4.2
---

# Handoff — выдача окна по макету, шаг 2.4.2

Продолжает [handoff дизайна выдачи](../2026-11-05-search-results-design/handoff.md) и [handoff схемы баллов](../2026-11-05-scores-implementation/handoff.md).

## Сделано

- **Выдача одним списком, код `ui/`, не собран и не запущен.** `search-results.ts` (новый): группировка разделов по `path`, разбор операторов для поля и чипов, подсветка слов запроса, разметка текстовых узлов для «Открыть раздел». `ResultCard.svelte` переписан в карточку документа: название и папка, два видимых раздела, «Ещё N разделов», раскрытие строки, «Открыть раздел», «Копировать ссылку», «В графе», «Похожие разделы», при «показать баллы» — число, полоска и разбор по сигналам. `SearchPane.svelte` переписан: поле с окрашенными операторами, «Понято как» с чипами «×», «?» с таблицей, параметры («Результатов», «показать баллы»), итог «N документов, M разделов», баннер ожидания модели с «Искать по словам сейчас», предупреждение «только по словам запроса», пустая выдача с «Без кавычек» / «Без исключения» / «Весь проект».
- **Состояние:** `app.svelte.ts` — `search({ wordsOnly })`, `searchWords`, `searchAs`, `setShowScores` (хранится в `localStorage`), `waitingModel`; убраны `textSearch`, `retrievalMode`, `includeRelated`, `graphSearch`, `minScore`, `emptyInfo`, диагностика пустой выдачи, «Порог N для этого запроса». `types.ts`: `ScoreKind` с `none`, `signals`, `parts`, `understood`, `applied.weights/signals`, `SearchDoc.min_score`. Настройки: одна `min_score` (`SettingsPage.svelte`, `settings-logic.ts`). `DocumentReference.terms` и `markTerms` в `WorkspaceDocument.svelte` — подсветка слов в разделе.
- **Убрано:** `TextResults.svelte`, `text-search.svelte.ts`, их тесты; тесты режимов (`main-navigation`, `labels-window`, `query-card`); стенд карточки запроса (`vite.query-card.config.ts`, `query-card-design.html`, `src/spikes/query-card/`); ветка `text_search` в `crates/bars-app/src/commands.rs`; `text_search` из `mock.ts`. `GraphSearchBundle` остался: его рисует панель графа. Вид запроса в графе строится по разделам выдачи без связанных источников (`PhysicalGraph.svelte`, `queryInput`).
- **Тесты, не запускались:** новые `search-results.test.ts` (группировка, операторы, подсветка), `SearchPane.test.ts` (нет режимов и «В тексте», чипы и «?», итог и баллы, `none`, ожидание модели, пустая выдача, Enter/IME), переписаны `ResultCard.test.ts`, `ResultCard.similar.test.ts`; правки `settings-logic.test.ts`, `Workspace.test.ts`, `project-switch.test.ts`, `graph-scope.test.ts`, `mock.test.ts`, `PhysicalGraph*.test.ts`, `new-topics-search.test.ts`.
- **Документы:** [дизайн глобального поиска](../../features/window/global-search-design.md) (проект → реализовано без проверки, режимы и «В тексте» убраны, раздел «Реализация выдачи, шаг 2.4.2»), [карточка запроса](../../features/window/query-card-design.md) (переписана), [главное окно](../../features/window/main-window.md), `labels.md`, `error-states.md`, [поиск «В тексте»](../../features/search/text-search.md), `window-parity.md`, руководство [`manual/search.md`](../../manual/search.md) и `troubleshooting.md`, оглавления окна и макетов; вопросы OQ-280 и OQ-281.

## Не сделано

- **Ничего не собиралось и не запускалось:** `npm`, `vitest`, `svelte-check`, `cargo`, `check.ps1`. Код не коммитился. Возможные места первой правки: типы `$derived`/`$state` в `ResultCard.svelte` и `SearchPane.svelte`, иконка `chevron-right` (рисуется в сетке 10×10 — размер подобран на глаз), селектор `[aria-label="Похожие разделы"] article button` в `ResultCard.similar.test.ts`.
- **«Искать по словам сейчас» пока не ускоряет поиск:** окно шлёт `wait_models: false`, `SearchParams` такого поля нет (OQ-280).
- Слова запроса в режиме исходника не подсвечиваются, только строки (OQ-281).
- Подсветка слов — по основе без стеммера воркера: «возврат» и «возврата» совпадут, редкие формы нет; точные смещения воркер не отдаёт.
- Не тронуты и ещё описывают прежний поиск: пробник `crates/bars-app/examples/query_card_probe.js`, `ui/global-search-design.html`, `docs/features/window/workspace-design.md` (строка Т-39), `document-actions.md` («вхождение «В тексте»»), `docs/knowledge/editor-spike.md`, прочие упоминания «В тексте» и режимов в `features/graphrag/`, `manual/graph.md`, `mcp-tools.md` (там «В тексте» как соответствие окну). Пункты выше — для сверки после проверки автором.
- Скриншотов обеих тем и 760 px нет: окно не запускалось.

## Сложности

- Рабочее дерево содержит чужие правки `ui/` (шаги 3.1.x), они сохранены. Дважды команда `git rm --cached` по ошибке сняла индекс каталога `ui/src/lib`; индекс восстановлен `git reset` без потери правок, удалённый по ошибке `GraphSearchBundle` возвращён `git checkout`.
- Из-за обмена кодировками в оболочке часть правок делалась через файлы-скрипты; тексты на русском проверены чтением.

## Дальше

Автор собирает и запускает `check.ps1` и `vitest` → правки по ошибкам → просмотр окна (обе темы, 760 px): поле с операторами и зеркалом, чипы, «?», карточки, баллы, состояния ожидания и пусто → ответ автора на OQ-280 (параметр воркера) и OQ-281 → сверка списка устаревших упоминаний выше.
