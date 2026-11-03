---
title: Handoff 3.1.2 — тексты окна, инструкции MCP и skills без предписанных модулей разработки
---

# Handoff 3.1.2 — тексты окна, инструкции MCP и skills без предписанных модулей разработки

Продолжение: [handoff 3.1.1](../2026-11-03-project-root-choice/handoff.md). Ничего не собиралось и не запускалось (запрет автора); код не коммитился.

## Сделано

- **Документы** (закоммичены): `features/agent-tools/module-scope.md`, `features/docs-convention/delivery.md`, `features/docs-convention/bars-instruction.md` (П-3), `manual/agents.md`, `manual/search.md`. Правило OQ-250: агент сам исключает журналы и архивы выбором модулей, человек выбирает область в окне; имена модулей — только настоящие папки проекта, `features/architecture/adr/chats` — совет для репозиториев кода.
- **Код без коммита:**
  - `crates/bars-mcp/src/instructions.rs`: строка о выборе модулей без предписанных имён и с правилом фильтра; список модулей добавляет назначение (`title` `Index.md`, до 40 знаков), пока помещается в 2000 знаков, иначе имя и число; тест на назначение добавлен.
  - `crates/bars-mcp/src/lib.rs`: описания `modules`, `files` и инструмента `bars_rag_search`, нейтральный пример `files`.
  - `crates/bars-cli/src/gate.rs`: текст отказа гейта.
  - `crates/bars-worker/assets/skills/bars-docs/SKILL.md`, `bars-session-log/SKILL.md`: правило фильтра, нейтральные примеры, таблица раскладки подана как совет для кода.
  - `ui/src/lib/SearchPane.svelte`: пример в поле запроса и совет «Снимите галочки с папок верхнего уровня»; `ui/src/lib/Sidebar.svelte`: подпись кнопки «Новая папка верхнего уровня»; `crates/bars-app/examples/tree_probe.js`: та же подпись.

## Не сделано

- Не запускались `cargo`, `npm`, тесты; правки не проверены, в том числе лимит 2000 знаков и сборка `instructions.rs`.
- Обе копии `BARS_INSTRUCTION.md` (`docs/BARS_INSTRUCTION.md`, `crates/bars-core/assets/BARS_INSTRUCTION.md`, строка 18 со списком `features … chats`) не тронуты по запрету: правило про модули нужно внести в шагах 3.1.3 / 3.1.4.
- HTML-мокапы окна (`docs/features/window/mockups/`) со старым примером «порог релевантности» не правились.
- Части skills про `log.md` — шаг 3.1.4.

## Сложности

Пример файлов в описаниях и гейте раньше брал `features/models/…`; заменён на нейтральный `instructions/…`, тесты на эти строки не найдены.

## Дальше

Общий прогон после разрешения автора: `cargo test -p bars-mcp -p bars-cli`, проверка окна; затем шаги 3.1.3 и 3.1.4 с правилом про модули в `BARS_INSTRUCTION.md`.
