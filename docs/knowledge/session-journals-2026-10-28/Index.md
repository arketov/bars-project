---
title: Данные проверки журналов сессий 28-10-2026
---

# Данные проверки журналов сессий 28-10-2026

[← База знаний](../Index.md)

Сырые данные к [проверке соглашения о журналах сессий](../session-journals.md). Скрипты запускаются Node.js без зависимостей.

| Файл | Что внутри |
|---|---|
| `journal-stats.mjs` | Счёт записей `log.md`, длины и разделов `handoff.md` по папке `docs/chats/`; аргумент — путь к папке |
| `journal-stats.txt` | Вывод `journal-stats.mjs` на 188 сессиях 28-10-2026 |
| `transcript-reads.mjs` | Счёт чтений чужих и своих handoff и log по расшифровкам Claude Code проекта в `~/.claude/projects/` |
| `transcript-reads.json` | Вывод `transcript-reads.mjs`: только счётчики, без текста расшифровок |
