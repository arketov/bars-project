---
title: Дизайн окна bars
---

# Дизайн окна bars

[← Оглавление раздела](../Index.md)

Как выглядит окно bars: описание экранов и HTML-мокапы. Мокапы не индексируются — bars берёт только `.md`.

Язык интерфейса и сообщений поиска — по НФТ-4. В текущем окне все пользовательские подписи — по-русски; язык инструкций агентам не влияет на язык окна.

- [refinement-plan.md](refinement-plan.md) — этапы 26–30 выполнены; меню папок реализовано, нативная приёмка этапов 31–32 открыта.
- [refinement-design.md](refinement-design.md) — этап 27: композиция и состояния уведомлений, анализа MCP и меню папок; интерактивные макеты.

- [main-window.md](main-window.md) — главный экран: выбор проекта, дерево документов с галочками, запрос и выдача.
- [../new-topics/](../new-topics/Index.md) — проект фичи «Новые темы»: первая строка дерева со своей иконкой, очередь вопросов и проверка материалов; пока без реализации.
- [../projects/deletion.md](../projects/deletion.md) — полное удаление проекта с подтверждением и сохранением документации.
- [mockups/delete-project.html](mockups/delete-project.html) — диалог необратимого удаления проекта.
- [settings-window.md](settings-window.md) — вкладки настроек и существующая сводка MCP; новый экран анализа ещё не подключён.
- [implementation.md](implementation.md) — как устроено окно: команда Tauri поверх обработчиков воркера, трей, один экземпляр, интерфейс на Svelte и что сделано сверх мокапов.
- [palette.md](palette.md) — палитра «Малина»: цвета светлой и тёмной темы.
- [reindex-consent.md](reindex-consent.md) — диалог согласия на переиндексацию после смены модели (Т-25).
- [indexing-progress.md](indexing-progress.md) — ход индексации: очередь, прогресс, ошибка файла, трей.
- [empty-states.md](empty-states.md) — пустые экраны: нет проектов, нет документации, нет результатов.
- [error-states.md](error-states.md) — ошибки: воркер, модель, провайдер, пропавший проект, переиндексация.

- [notifications.md](notifications.md) — этап 20 и целевой контракт этапа 26: операции, достоверные итоги, доставка, внимание, сценарии N-01…16.

- [document-actions.md](document-actions.md) — меню файлов и папок, реализация этапа 31, цели и проверки F-01…11; ограничения нативной приёмки.

- [logs.md](logs.md) — этап 19: файловый журнал, уровни, ротация и просмотр в настройках.

## Мокапы

- [mcp-statistics.md](mcp-statistics.md) — экран отдельных вызовов, данные/хранение MCP, API фильтров/курсора/подсказок/управления; сценарии M-01…07 и границы проверки.

- [project-menu.md](project-menu.md) — этап 23: меню проекта, проверка индекса и граница полного удаления.

[Полное оглавление макетов](mockups/Index.md).

- [mockups/logs.html](mockups/logs.html) — вкладка логов с фильтром уровня.

Открываются двойным кликом: внешних загрузок нет, стили и скрипты лежат рядом отдельными файлами. Мокапы состояний переключают сцены кнопками вверху страницы.

- [mockups/notifications.html](mockups/notifications.html) — центр уведомлений: загрузка, индексация и ошибка.
- [mockups/document-menu.html](mockups/document-menu.html) — документ из дерева и меню с подменю редакторов.
- [mockups/main.html](mockups/main.html) — главный экран.
- [mockups/settings.html](mockups/settings.html) — экран настроек.
- [mockups/device-settings.html](mockups/device-settings.html) — проект этапа 17: отдельные устройства моделей, фактическое исполнение и ошибки индексации; восемь сцен.
- [mockups/theme.css](mockups/theme.css) — палитра и общие элементы.
- [mockups/theme.js](mockups/theme.js) — тема окна: как в системе, светлая, тёмная.
- [mockups/reindex-consent.html](mockups/reindex-consent.html) — диалог согласия на переиндексацию: встроенная модель, удалённый провайдер, окно после «Позже».
- [mockups/indexing.html](mockups/indexing.html) — ход индексации: переиндексация, обычное обновление, ошибка файла, трей.
- [mockups/empty.html](mockups/empty.html) — пустые экраны.
- [mockups/errors.html](mockups/errors.html) — экраны ошибок.
- [mockups/states.css](mockups/states.css), [mockups/states.js](mockups/states.js) — общее для мокапов состояний: боковая панель, баннеры, диалог, переключатель сцен.
