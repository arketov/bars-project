//! Протокол локального канала между воркером и клиентами (`bars-mcp`, `bars-cli`).
//!
//! Типы и кодек: JSON-RPC 2.0 по строке на сообщение, методы с параметрами и ответами, ошибки,
//! рукопожатие `hello`. Транспорт: адрес канала ([`Endpoint`]), чтение и запись строк, клиент воркера
//! ([`Client`]) и цикл соединения для воркера ([`serve_connection`]). Слушатель канала с ограничением
//! доступа живёт в `bars-app`, запуск воркера по требованию — позже.
//! Контракт — `docs/architecture/protocol.md` и `docs/architecture/ipc.md`.

mod client;
mod endpoint;
mod error;
mod framing;
mod jsonrpc;
mod launch;
mod methods;
mod server;
mod session;
mod text;
#[cfg(windows)]
pub mod winsid;

pub use client::{Client, ClientError};
pub use endpoint::Endpoint;
pub use error::*;
pub use framing::{MAX_REQUEST_LINE, MAX_RESPONSE_LINE, read_line_limited, write_line};
pub use jsonrpc::*;
pub use launch::{
    LAUNCH_DEADLINE, connect_or_launch, connect_or_launch_with, spawn_worker, worker_exe,
};
pub use methods::*;
pub use server::{Handler, serve_connection};
pub use session::Session;
pub use text::number_lines;

/// Версия протокола. Клиент и воркер сверяют её в `hello`; любое несовместимое изменение методов
/// увеличивает её.
pub const PROTOCOL_VERSION: u32 = 1;
