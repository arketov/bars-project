---
title: Дизайн окна bars
---

# Дизайн окна bars

Как выглядит окно bars: описание экранов и HTML-мокапы. Мокапы не индексируются — bars берёт только `.md`.

Язык интерфейса и сообщений поиска — по НФТ-4. В текущем окне все пользовательские подписи — по-русски; язык инструкций агентам не влияет на язык окна.

- [refinement-plan.md](refinement-plan.md) — открытые доработки уведомлений, анализа MCP и меню папок: отдельный дизайн UX/UI, данные, реализация и приёмка.

- [main-window.md](main-window.md) — главный экран: выбор проекта, дерево документов с галочками, запрос и выдача.
- [../projects/deletion.md](../projects/deletion.md) — полное удаление проекта с подтверждением и сохранением документации.
- [mockups/delete-project.html](mockups/delete-project.html) — диалог необратимого удаления проекта.
- [settings-window.md](settings-window.md) — экран настроек: модели, поиск, нарезка, установка в клиенты, оформление.
- [implementation.md](implementation.md) — как устроено окно: команда Tauri поверх обработчиков воркера, трей, один экземпляр, интерфейс на Svelte и что сделано сверх мокапов.
- [palette.md](palette.md) — палитра «Малина»: цвета светлой и тёмной темы.
- [reindex-consent.md](reindex-consent.md) — диалог согласия на переиндексацию после смены модели (Т-25).
- [indexing-progress.md](indexing-progress.md) — ход индексации: очередь, прогресс, ошибка файла, трей.
- [empty-states.md](empty-states.md) — пустые экраны: нет проектов, нет документации, нет результатов.
- [error-states.md](error-states.md) — ошибки: воркер, модель, провайдер, пропавший проект, переиндексация.

- [notifications.md](notifications.md) — этап 20 и целевой контракт этапа 26: операции, достоверные итоги, доставка, внимание, сценарии N-01…16.

- [document-actions.md](document-actions.md) — этап 18 и контракт этапа 26: цели корня/модуля/папки, действия и сценарии F-01…11.

- [logs.md](logs.md) — этап 19: файловый журнал, уровни, ротация и просмотр в настройках.

## Мокапы

- [mcp-statistics.md](mcp-statistics.md) — этап 21 и контракт этапа 26: отдельные попытки шести инструментов, 7 дней истории, параметры, результаты, сценарии M-01…07.

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
