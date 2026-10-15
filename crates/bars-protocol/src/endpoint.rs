//! Адрес локального канала воркера (`docs/architecture/ipc.md`).

use std::io;
use std::path::PathBuf;

use interprocess::local_socket::{GenericFilePath, GenericNamespaced, Name, ToFsName, ToNsName};

/// Где воркер слушает, а клиенты подключаются.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    /// Имя в пространстве имён ОС: `\\.\pipe\<имя>` на Windows.
    Namespaced(String),
    /// Файл Unix-сокета.
    Path(PathBuf),
}

impl Endpoint {
    /// Канал текущего пользователя: `\\.\pipe\bars-<имя пользователя>` на Windows,
    /// `~/.bars/run/bars.sock` на macOS и других Unix.
    pub fn current() -> io::Result<Self> {
        if cfg!(windows) {
            let user = std::env::var("USERNAME").unwrap_or_default();
            // Подменённая домашняя папка — отдельный экземпляр bars со своим каналом.
            Ok(match crate::home::home_override() {
                Some(home) => {
                    Self::named(&format!("{user}-{:08x}", fnv1a(&home.to_string_lossy())))
                }
                None => Self::named(&user),
            })
        } else {
            let home = crate::home::user_home().ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "не найдена домашняя папка")
            })?;
            Ok(Self {
                kind: Kind::Path(home.join(".bars").join("run").join("bars.sock")),
            })
        }
    }

    /// Канал `bars-<суффикс>` в пространстве имён ОС. Для тестов: у каждого теста свой суффикс.
    /// Всё, кроме `[A-Za-z0-9._-]`, в суффиксе заменяется на `_`.
    pub fn named(suffix: &str) -> Self {
        let clean: String = suffix
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        Self {
            kind: Kind::Namespaced(format!("bars-{clean}")),
        }
    }

    /// Канал в файле: для Unix и для тестов.
    pub fn at_path(path: impl Into<PathBuf>) -> Self {
        Self {
            kind: Kind::Path(path.into()),
        }
    }

    /// Файл сокета, если адрес файловый. Воркер создаёт для него папку с правами `0700`.
    pub fn socket_path(&self) -> Option<&std::path::Path> {
        match &self.kind {
            Kind::Path(path) => Some(path),
            Kind::Namespaced(_) => None,
        }
    }

    /// Имя для `interprocess`.
    pub fn name(&self) -> io::Result<Name<'static>> {
        match &self.kind {
            Kind::Namespaced(name) => name.clone().to_ns_name::<GenericNamespaced>(),
            Kind::Path(path) => path.clone().to_fs_name::<GenericFilePath>(),
        }
    }
}

/// Короткий хеш строки для имени канала подменённой домашней папки.
fn fnv1a(text: &str) -> u32 {
    text.bytes().fold(0x811c_9dc5, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffix_is_cleaned_of_path_characters() {
        assert_eq!(
            Endpoint::named(r"DOM\иван пётр"),
            Endpoint::named(&format!("DOM{}", "_".repeat(10)))
        );
        assert_eq!(Endpoint::named("a.b_c-d"), Endpoint::named("a.b_c-d"));
    }

    #[test]
    fn current_endpoint_matches_the_platform() {
        let endpoint = Endpoint::current().unwrap();
        assert_eq!(endpoint.socket_path().is_some(), !cfg!(windows));
        if let Some(path) = endpoint.socket_path() {
            assert!(path.ends_with(".bars/run/bars.sock"));
        }
        endpoint.name().unwrap();
    }
}
