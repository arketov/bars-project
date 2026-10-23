//! Воркер bars без окна: канал, обработчики методов, реестр, очередь. Окно и трей (`bars-app`) строятся
//! поверх него; интеграционные тесты `bars-cli` и `bars-mcp` поднимают настоящий воркер с подделкой
//! модели (`test-support`). Библиотека отдельно от `bars-app`: сборочный скрипт Tauri ломает линковку тестов зависимых крейтов.

pub mod audits;
pub mod autostart;
pub mod clients;
pub mod handler;
pub mod ipc;
pub mod keybindings;
pub mod logging;
pub mod mcp_history;
pub mod mcp_statistics;
pub mod new_topics;
pub mod serve;
pub mod settings_io;
pub mod text_search;
pub mod worker;
