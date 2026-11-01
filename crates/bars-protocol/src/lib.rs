//! Протокол локального канала между воркером и клиентами (`bars-mcp`, `bars-cli`).
//!
//! Типы и кодек: JSON-RPC 2.0 по строке на сообщение, методы с параметрами и ответами, ошибки,
//! рукопожатие `hello`. Транспорт: адрес канала ([`Endpoint`]), чтение и запись строк, клиент воркера
//! ([`Client`]) и цикл соединения для воркера ([`serve_connection`]). Слушатель канала с ограничением
//! доступа живёт в `bars-worker`, запуск воркера по требованию — позже.
//! Контракт — `docs/architecture/protocol.md` и `docs/architecture/ipc.md`.

mod audits;
mod client;
mod disk_read;
mod endpoint;
mod error;
mod framing;
mod graph;
pub mod graph_text;
mod home;
mod instruction;
mod jsonrpc;
mod launch;
mod links;
mod methods;
mod new_topics;
mod operations;
mod search_scope;
mod server;
mod session;
mod text;
mod text_search;
#[cfg(windows)]
pub mod winsid;

pub use audits::*;
pub use bars_graph as graph_types;
pub use client::{Client, ClientError};
pub use disk_read::*;
pub use endpoint::Endpoint;
pub use error::*;
pub use framing::{MAX_REQUEST_LINE, MAX_RESPONSE_LINE, read_line_limited, write_line};
pub use graph::*;
pub use home::{USER_HOME_VAR, home_override, user_home};
pub use instruction::{agent_install_text, mcp_config_json};
pub use jsonrpc::*;
pub use launch::{
    LAUNCH_DEADLINE, connect_or_launch, connect_or_launch_with, spawn_worker, worker_exe,
};
pub use links::*;
pub use methods::*;
pub use new_topics::*;
pub use operations::*;
pub use search_scope::search_scope_text;
pub use server::{Handler, serve_connection};
pub use session::Session;
pub use text::number_lines;
pub use text_search::*;

/// Версия протокола. Клиент и воркер сверяют её в `hello`; любое несовместимое изменение методов
/// увеличивает её.
pub const PROTOCOL_VERSION: u32 = 1;

/// Идентификатор сборки (коммит, из которого собран exe), `unknown` без git. Воркер отдаёт его в
/// `hello`, клиент сверяет со своим и предупреждает о расхождении (`docs/architecture/protocol.md`).
pub const BUILD_ID: &str = env!("BARS_BUILD_ID");
