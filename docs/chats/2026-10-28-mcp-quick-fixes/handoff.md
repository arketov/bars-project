---
title: Итог сессии 4.1.1 — быстрые исправления MCP
---

# Итог сессии 4.1.1 — быстрые исправления MCP

## Сделано

Шаг плана 4.1.1, записи ISS-27, ISS-28, ISS-31, ISS-88 в реестре проблем закрыты.

- **ISS-27, описания.** У `bars_get_similar` убраны серии по 24 пробела. Поля `mode`, `include_related`, `related_budget`, `graph_budgets` описаны через действие. Описание каждой из шести графовых операций начинается с задачи, для которой её вызывать. Тест списка инструментов не пропускает двойные пробелы в описаниях и схемах. Контракт — [инструменты MCP](../../features/agent-tools/mcp-tools.md).
- **ISS-28, выдача.** В `APPLIED PARAMETERS` поиска, similar и duplicates вместо `results:` теперь `limit:` и `returned:`; в CLI — «предел результатов N, выдано M». Новое поле протокола `SearchHit.sections` содержит заголовки разделов, приклеенных к чанку; MCP печатает их строкой `sections:`, CLI — «и разделы:». Описано в `architecture/protocol.md`.
- **ISS-88.** Если пакет не помещается в `related_budget` даже пустым, mix отвечает обычной выдачей с `applied_mode: hybrid` и `fallback_reason` о `related_budget`. Раньше была ошибка о `max_bytes`. Константа `ENVELOPE_DOES_NOT_FIT` в `bars-graph/src/query.rs`. Описано в `graphrag/implementation.md` и `graphrag/tools.md`.
- **ISS-31, история.** Прокси передаёт исходный вход графовых операций. Воркер сохраняет query, entity_ref, from, to, roots, topic, analysis, budgets, область и другие основные аргументы; в `applied` пишет операцию и область. Номер попытки прокси при `model_loading` — `applied.proxy_attempt`. Окно подписывает новые поля. Контракт — [статистика MCP](../../features/window/mcp-statistics.md).
- Код: `crates/bars-mcp/src/{lib,graph,observation,render}.rs`, `crates/bars-worker/src/{mcp_history.rs,handler/history.rs,handler/mod.rs}`, `crates/bars-protocol/src/methods.rs`, `crates/bars-cli/src/render.rs`, `ui/src/lib/mcp-history.ts`, тесты рядом.
- Проверки: `scripts/check.ps1` зелёный; новые тесты — номер попытки прокси, история шести графовых операций через настоящий MCP, `related_budget: 1`, разделы чанка, рендер `limit/returned/sections`.

## Не сделано

- Установленная сборка и настоящие клиенты не проверялись: новые описания дойдут до агентов после установки (4.4.3).
- Отдельные описания полей `GraphArgs` не добавлялись: схема у каждой операции своя будет в 4.3.3 (ISS-23), а лишний текст в общей схеме увеличил бы список инструментов.
- В `sections` может попасть заголовок из текста, скопированного «сверху» в малый чанк (правило добора нарезки). Случай редкий, не исправлялся.

## Сложности

- В том же checkout параллельно работали другие сессии. Статусы ISS ушли в чужой коммит `632b0d2` раньше кода. В коммит 4.1.1 я брал только свои фрагменты общих файлов.
- Тесты одного `bars-worker` не собираются отдельно от workspace (`graph_cache` требует serde `rc` от других крейтов): целевые тесты нужно запускать с `--workspace`.

## Дальше

4.1.2 — скрыть графовые инструменты по умолчанию (OQ-162); 4.1.3 — тексты skills и BARS_INSTRUCTION (ISS-29, ISS-30).
