---
title: Воспроизводимый стенд физики графа
---

# Воспроизводимый стенд физики графа

Исследовательский код; не подключён к приложению. [Метод и выводы](../../docs/knowledge/graph-render.md).

Node 24, npm, существующий Chromium и Playwright из runtime Codex. В этой папке выполнить `npm ci --ignore-scripts`, затем:

```powershell
node --test export.test.mjs
node measure.mjs <новая-папка-доказательств> <каталог-node_modules-с-playwright> <существующий-chrome.exe> [snapshots.json.gz]
node capture.mjs <папка-доказательств> <каталог-node_modules-с-playwright> <существующий-chrome.exe>
node verify.mjs <папка-доказательств>
```

Без последнего аргумента measure читает `.bars/index.sqlite` текущего bars-project и соседнего `cyberProject/ai-mutiplayer-bots` только для чтения. С последним аргументом повторяет сохранённые проекции без доступа к индексам. Выходная папка должна быть новой; незавершённый прогон сохраняется с отсутствующим `success`, не считается успешным. `capture` читает только сохранённый снимок. Никаких автоматических установок браузера, моделей или изменений пользовательского профиля.

Файлы: `export.mjs` — read-only снимок и проекции; `export.test.mjs` — неизменность SQLite и разделение типов; `browser.mjs` — физика, рендер и контролируемый опыт; `worker.mjs` — порционные вычисления d3; `measure.mjs` — измерения/манифест; `capture.mjs` — снимки раскладок; `verify.mjs` — целостность доказательств; `package-lock.json` — закреплённые зависимости стенда. `dist/`, `dist-capture/`, `node_modules/` — локальные выходы, не доказательства.
