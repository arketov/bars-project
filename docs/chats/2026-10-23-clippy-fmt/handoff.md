# Передача: fmt и clippy во всём workspace (ISS-63)

## Сделано

- `cargo fmt --all` применён; `cargo fmt --all --check` чист.
- `cargo clippy --workspace --all-targets -- -D warnings` зелёный при закоммиченном `bars-worker/src/lib.rs`. Правки: `bars-graph` (`if` через let-chain, `allow(too_many_arguments)` на `edge` и `walk`), `bars-protocol` и `bars-cli` (`allow(large_enum_variant)` на `Call` и `GraphAction`), `bars-core` (`as_chunks`, диапазон в `vec!`, тестовый модуль `builtin.rs` в конец файла, примеры), `bars-worker` (тип `DeferCheck`, `from_ref` в тестах, примеры), `bars-app` (`assert!`, `allow` в тесте, недостающие модули в четырёх примерах).
- Запись ISS-63 перенесена в «Исправленные проблемы» в `docs/backlog/issues.md`; в [build.md](../../knowledge/build.md) добавлен способ короткого пути через `subst`.

## Не сделано

- `cargo build` и `cargo test` не запускались.
- Замечания `dead_code` в `bars-worker` (`len`, `is_empty`, поле `ReindexRequired`) видны, только когда снято `allow(dead_code)` в `lib.rs`, это пункт 1.1.2; правка `lib.rs` и `model_lifecycle.rs` чужая, в коммит не вошла.

## Сложности

- Длинный путь репозитория ломает `llama-cpp-sys-2`; помогла буква `R:` через `subst` вместо `target` вне репозитория.
- Мои запуски с `C:/bt/i111` были лишними и запрещены автором; каталог и логи удалены.

## Дальше

Пункт 1.1.2 (ISS-80) начинается с трёх замечаний `dead_code`; пункт 1.1.3 — тест выгрузки модели.
