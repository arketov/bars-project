---
title: Дизайн окна bars
---

# Дизайн окна bars

Как выглядит окно bars: описание экранов и HTML-мокапы. Мокапы не индексируются — bars берёт только `.md`.

- [main-window.md](main-window.md) — главный экран: выбор проекта, дерево документов с галочками, запрос и выдача.
- [settings-window.md](settings-window.md) — экран настроек: модели, поиск, нарезка, установка в клиенты, оформление.
- [implementation.md](implementation.md) — как устроено окно: команда Tauri поверх обработчиков воркера, трей, один экземпляр, интерфейс на Svelte и что сделано сверх мокапов.
- [palette.md](palette.md) — палитра «Малина»: цвета светлой и тёмной темы.
- [reindex-consent.md](reindex-consent.md) — диалог согласия на переиндексацию после смены модели (Т-25).
- [indexing-progress.md](indexing-progress.md) — ход индексации: очередь, прогресс, ошибка файла, трей.
- [empty-states.md](empty-states.md) — пустые экраны: нет проектов, нет документации, нет результатов.
- [error-states.md](error-states.md) — ошибки: воркер, модель, провайдер, пропавший проект, переиндексация.

## Мокапы

Открываются двойным кликом: внешних загрузок нет, стили и скрипты лежат рядом отдельными файлами. Мокапы состояний переключают сцены кнопками вверху страницы.

- [mockups/main.html](mockups/main.html) — главный экран.
- [mockups/settings.html](mockups/settings.html) — экран настроек.
- [mockups/theme.css](mockups/theme.css) — палитра и общие элементы.
- [mockups/theme.js](mockups/theme.js) — тема окна: как в системе, светлая, тёмная.
- [mockups/reindex-consent.html](mockups/reindex-consent.html) — диалог согласия на переиндексацию: встроенная модель, удалённый провайдер, окно после «Позже».
- [mockups/indexing.html](mockups/indexing.html) — ход индексации: переиндексация, обычное обновление, ошибка файла, трей.
- [mockups/empty.html](mockups/empty.html) — пустые экраны.
- [mockups/errors.html](mockups/errors.html) — экраны ошибок.
- [mockups/states.css](mockups/states.css), [mockups/states.js](mockups/states.js) — общее для мокапов состояний: боковая панель, баннеры, диалог, переключатель сцен.
