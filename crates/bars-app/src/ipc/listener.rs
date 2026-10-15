//! Слушатель канала с ограничением доступа.

use std::io;

use interprocess::local_socket::ListenerOptions;
use interprocess::local_socket::tokio::Listener;

use bars_protocol::Endpoint;

#[derive(Debug, thiserror::Error)]
pub enum BindError {
    /// Имя канала занято: воркер уже работает (Т-1).
    #[error("воркер bars уже запущен")]
    AlreadyRunning,
    #[error("не удалось открыть канал: {0}")]
    Io(#[from] io::Error),
}

/// Открывает канал, к которому может подключиться только текущий пользователь.
///
/// - Windows: именованный канал с защищённым DACL из одной записи — SID текущего пользователя.
///   Занятое имя Windows сообщает как `PermissionDenied` (os error 5), а не `AddrInUse`.
/// - Unix: сокет `0600` в папке `0700`; сокет, оставшийся от упавшего воркера, удаляется, сокет
///   живого воркера — нет. Ветка Unix на macOS не проверена: нужен Mac.
pub fn bind(endpoint: &Endpoint) -> Result<Listener, BindError> {
    #[cfg(unix)]
    unix::prepare(endpoint)?;
    let options = ListenerOptions::new().name(endpoint.name()?);
    #[cfg(windows)]
    let options = {
        use interprocess::os::windows::local_socket::ListenerOptionsExt;
        options.security_descriptor(super::acl_windows::owner_only_descriptor()?)
    };
    let listener = options.create_tokio().map_err(|error| match error.kind() {
        io::ErrorKind::AddrInUse => BindError::AlreadyRunning,
        io::ErrorKind::PermissionDenied if cfg!(windows) => BindError::AlreadyRunning,
        _ => BindError::Io(error),
    })?;
    #[cfg(unix)]
    unix::restrict(endpoint)?;
    Ok(listener)
}

#[cfg(unix)]
mod unix {
    use std::fs;
    use std::io;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::os::unix::net::UnixStream;

    use bars_protocol::Endpoint;

    use super::BindError;

    /// Папка сокета с правами `0700` и удаление мёртвого сокета.
    pub fn prepare(endpoint: &Endpoint) -> Result<(), BindError> {
        let Some(path) = endpoint.socket_path() else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)?;
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        }
        if path.exists() {
            match UnixStream::connect(path) {
                Ok(_) => return Err(BindError::AlreadyRunning),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                    ) =>
                {
                    fs::remove_file(path)?;
                }
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    /// Права `0600` на созданный сокет. Папка `0700` закрывает окно между созданием и сменой прав.
    pub fn restrict(endpoint: &Endpoint) -> Result<(), BindError> {
        if let Some(path) = endpoint.socket_path() {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[tokio::test]
    async fn pipe_dacl_allows_only_the_current_user() {
        use std::os::windows::io::AsRawHandle;
        let endpoint = Endpoint::named("dacl-check");
        let _listener = bind(&endpoint).unwrap();
        let pipe = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(r"\\.\pipe\bars-dacl-check")
            .unwrap();
        super::super::acl_windows::assert_owner_only(pipe.as_raw_handle());
    }

    #[tokio::test]
    async fn taken_name_is_already_running() {
        let endpoint = Endpoint::named("taken-name");
        let _first = bind(&endpoint).unwrap();
        assert!(matches!(bind(&endpoint), Err(BindError::AlreadyRunning)));
    }
}
