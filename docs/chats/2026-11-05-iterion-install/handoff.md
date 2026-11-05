---
title: Handoff — клиент iterion в установщике bars (шаг 4.1.3)
---

# Handoff — клиент iterion в установщике bars

## Сделано

- Клиент `iterion` добавлен в `InstallClient` (протокол), `ClientArg` CLI, список окна и фоновый проход; новый модуль `crates/bars-worker/src/clients/iterion.rs`.
- Установка пишет `<ITERION_HOME или ~/.iterion>/plugins/bars/plugin.yaml` (только поля, известные строгому разбору iterion) и `skills/bars-docs/SKILL.md`; `plugins.yaml` не трогается, хука нет. Удаление убирает папку `plugins/bars` целиком. Признак установки для автообновления — существующий `plugin.yaml`.
- `InstallEnv` получил поле `iterion_home` (из `ITERION_HOME` в `detect`); во всех тестовых литералах стоит `None`.
- Окно: имя и подпись iterion в «Настройки → Клиенты», макет, тип `InstallClientName`. Текст `--print-instruction` дополнен строкой про iterion.
- Тесты написаны и не запускались: три в `clients/tests.rs`, один в `clients/refresh_tests.rs`.
- Документы: `clients.md` (статус, путь skill, признак установки, автообновление), `manual/agents.md`, `settings-window.md`, `cli.md`.

## Не сделано

- Сборка, clippy и тесты не запускались (запрет автора); код не проверен компилятором.
- Настоящий iterion не запускался: чтение манифеста и skill его загрузчиком не проверено.

## Дальше

- Проверить сборкой: `cargo check`, тесты `clients::`, `npm run check` — по слову автора.
- В `docs/features/docs-convention/delivery.md` слова про skills клиентов не менялись: если там считаются файлы skills по клиентам, добавить, что iterion получает только `bars-docs`.
