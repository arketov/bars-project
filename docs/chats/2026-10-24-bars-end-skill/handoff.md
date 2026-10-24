---
title: Handoff — skill bars-end для завершения сессии
---

# Handoff — skill bars-end

## Сделано

- Спроектирован и встроен третий skill `bars-end`: агент по команде пользователя переносит итоги сессии в документацию. Решения OQ-184…OQ-189 (24-10-2026): приняты рекомендации агента.
- Контракт — [завершение сессии](../../features/docs-convention/session-end.md); поправлены `delivery.md`, `session-log.md`, `clients.md`, `manual/agents.md`, `Index.md` фичи, CHANGELOG. Коммит `6d41ca5`, запушен в `main`.
- Текст: `crates/bars-worker/assets/skills/bars-end/SKILL.md`; строка в `SKILLS` (`clients/mod.rs`), тесты установки и удаления, списки skills в скриптах проверок.
- По просьбе автора skill скопирован вручную в `~/.claude/skills/bars-end/`; первый прогон `/bars-end` — в этой же сессии. По его итогам поправлены `delivery.md`, `session-end.md` и [сборка и проверки](../../knowledge/build.md).

## Не сделано

- Пункта плана нет: автор попросил сделать сразу.
- Установленный bars старый (запрет OQ-113): штатно `bars-end` появится у клиентов только после установки новой сборки; в `~/.agents/skills` (Codex) копии нет.
- `scripts/test-docs-agent-sessions.mjs` не дополнен: он проверяет ведение документации, а не завершение сессии.

## Сложности

- Три теста считают файлы установки, и с третьим skill упали. Решено правкой чисел; где что менять при новом skill — в [доставке](../../features/docs-convention/delivery.md).
- В `QUESTIONS.md` лежали чужие незакоммиченные правки вперемешку со своими. Закоммичена версия «HEAD плюс свои вставки» через `git hash-object` и `git update-index`; способ — в [сборке и проверках](../../knowledge/build.md).
- `perl -CSD` молча не заменил кириллицу (нет `use utf8`), правка сделана через node.
- У установленного `bars-mcp` нет `bars_text_search` и `bars_links`, хотя skills их советуют: старая сборка. Точный поиск заменён Grep.
- В Codex нет поля `disable-model-invocation`: запрет самостоятельного вызова там держится только на описании.

## Дальше

- Обкатать `/bars-end` на сессии с кодом и в Codex, поправить текст по итогам.
