---
title: Handoff — клиент Kimi Code
---

# Handoff: клиент Kimi Code (автонастройка, Т-21)

## Сделано

- Клиент `kimi-code` как полный паритет с Claude Code и Codex (решение автора 31-10-2026 по OQ-219…222): MCP-сервер в `~/.kimi-code/mcp.json`, хук начала сессии записью `[[hooks]]` в `~/.kimi-code/config.toml` (новый модуль `kimi_toml.rs`, toml_edit, только разрешённые поля event/matcher/command), skills в свою папку `~/.kimi-code/skills/`, строка в окне, фоновое автообновление (проход Claude Code → Codex → Kimi Code), uninstall, инструкция агенту, деинсталлятор NSIS. Код: `crates/bars-protocol/src/methods.rs`, `crates/bars-cli/src/lib.rs`, `crates/bars-worker/src/clients/` (mod.rs, refresh.rs, kimi_toml.rs), `handler/mod.rs`, `ui/src/lib/` (types.ts, SettingsPage.svelte, mock.ts), `installer-hooks.nsh`.
- Тесты: unit-кейсы установки/идемпотентности/uninstall/неразбираемого конфига, refresh-тесты на трёх клиентах, интеграционный тест CLI, стенд `scripts/test-client-refresh.ps1` расширен (фикстуры, снимки, uninstall, пустой профиль). Починены два теста, жёстко завязанных на двух клиентов (`handler/tests/window.rs`, `refresh_tests.rs`).
- Документы: [clients.md](../../features/clients/clients.md) (раздел Kimi Code, форматы, автообновление, что не подтверждено), `manual/agents.md`, `settings-window.md`, `cli.md`, `README.md`. OQ-219…222 закрыты в `QUESTIONS.md`.
- Проверка: полный `scripts/check.ps1` зелёный 31-10-2026 (fmt, clippy, build, cargo test, UI check, 582 UI-теста).

## Не сделано

- Тексты Т-21, Т-28 и критериев приёмки в MAIN_IDEA.md и пункт PLAN.md — правка за автором (правило проекта).
- Гейт чтения документации на Kimi Code не переносился (OQ-222: отдельным шагом). Permission-правило `mcp__bars__*` не добавляется (OQ-221).
- Ручная проверка на настоящей сессии Kimi Code (Desktop или CLI) не делалась; `KIMI_CODE_HOME` не поддерживается — записано в «Что не подтверждено» clients.md.

## Сложности

- `cargo test -p bars-worker` падает компиляцией теста `graph_cache.rs` (serde без фичи rc); штатный прогон workspace-wide, как в check.ps1. Не дефект.
- В оболочке агента нет окружения сборки: VULKAN_SDK и LIBCLANG_PATH из build.md пришлось задавать вручную.

## Дальше

- Автор: правка Т-21/Т-28/приёмки и PLAN.md, установка новой сборки (OQ-217).
- Внимание: на машине автора `~/.kimi-code/mcp.json` уже содержит ручную запись `bars`: фоновое автообновление новой сборки нормализует её и добавит хук SessionStart в настоящий `config.toml` — это поведение фичи, после установки стоит проверить `[[hooks]]` в `~/.kimi-code/config.toml` и перезапустить клиент.
