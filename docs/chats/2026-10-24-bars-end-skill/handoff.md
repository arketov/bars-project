---
title: Handoff — skill bars-end для завершения сессии
---

# Handoff — skill bars-end

## Сделано

- Спроектирован и встроен третий skill `bars-end`: агент по команде пользователя переносит итоги сессии в документацию. Решения OQ-184…OQ-189 (24-10-2026): приняты рекомендации агента.
- Контракт — [завершение сессии](../../features/docs-convention/session-end.md); поправлены `delivery.md`, `session-log.md`, `clients.md`, `manual/agents.md`, `Index.md` фичи, CHANGELOG.
- Текст: `crates/bars-worker/assets/skills/bars-end/SKILL.md`; строка в `SKILLS` (`clients/mod.rs`), тесты установки и удаления, списки skills в скриптах проверок.

## Не сделано

- Пункта плана нет: автор попросил сделать сразу. Если нужен пункт задним числом — предложить автору.
- Установленный bars старый (запрет OQ-113), поэтому `bars-end` появится в `~/.claude/skills` и `~/.agents/skills` только после установки новой сборки.
- `scripts/test-docs-agent-sessions.mjs` не дополнен: он проверяет ведение документации, а не завершение сессии.

## Сложности

- В Codex нет поля `disable-model-invocation`: запрет самостоятельного вызова там держится только на описании skill.

## Дальше

- Обкатать `/bars-end` на настоящей сессии и поправить текст по итогам.
