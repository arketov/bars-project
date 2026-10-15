//! Ключи провайдеров: хранилище ОС и переменная окружения (`docs/architecture/secrets.md`).
//!
//! В настройках лежит только имя записи; сам ключ читается из хранилища в момент запроса к
//! провайдеру и нигде не пишется: ни в логи, ни в сообщения об ошибках.

use std::sync::Arc;

use keyring_core::{CredentialStore, Error as KeyringError};

/// Имя сервиса записей bars в хранилище ОС.
pub const KEYRING_SERVICE: &str = "bars";

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("хранилище ключей ОС недоступно: {0}")]
    Store(String),
    #[error("не удалось прочитать ключ `{name}` из хранилища ОС: {reason}")]
    Read { name: String, reason: String },
    #[error("не удалось записать ключ `{name}` в хранилище ОС: {reason}")]
    Write { name: String, reason: String },
}

/// Хранилище ключей. Ключ не попадает ни в ошибки, ни в `Debug`.
pub trait SecretStore: Send + Sync {
    /// Ключ по имени записи; `None`, если записи нет.
    fn get(&self, name: &str) -> Result<Option<String>, SecretError>;
    fn set(&self, name: &str, key: &str) -> Result<(), SecretError>;
    /// Удаляет запись; отсутствие записи не ошибка.
    fn delete(&self, name: &str) -> Result<(), SecretError>;
}

/// Хранилище ОС через `keyring-core`: Credential Manager на Windows, Keychain на macOS.
pub struct KeyringSecrets {
    store: Arc<CredentialStore>,
}

impl KeyringSecrets {
    /// Хранилище текущей ОС.
    pub fn os() -> Result<Self, SecretError> {
        Ok(Self::with_store(os_store()?))
    }

    /// Любое хранилище `keyring-core`; тесты берут его `mock`.
    pub fn with_store(store: Arc<CredentialStore>) -> Self {
        Self { store }
    }

    fn entry(&self, name: &str) -> Result<keyring_core::Entry, KeyringError> {
        self.store.build(KEYRING_SERVICE, name, None)
    }
}

#[cfg(windows)]
fn os_store() -> Result<Arc<CredentialStore>, SecretError> {
    windows_native_keyring_store::Store::new()
        .map(|store| store as Arc<CredentialStore>)
        .map_err(|error| SecretError::Store(error.to_string()))
}

#[cfg(target_os = "macos")]
fn os_store() -> Result<Arc<CredentialStore>, SecretError> {
    apple_native_keyring_store::keychain::Store::new()
        .map(|store| store as Arc<CredentialStore>)
        .map_err(|error| SecretError::Store(error.to_string()))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn os_store() -> Result<Arc<CredentialStore>, SecretError> {
    Err(SecretError::Store(
        "bars хранит ключи только в Windows и macOS; на этой ОС используйте `api_key_env`".into(),
    ))
}

impl SecretStore for KeyringSecrets {
    fn get(&self, name: &str) -> Result<Option<String>, SecretError> {
        let read = |error: KeyringError| SecretError::Read {
            name: name.to_string(),
            reason: error.to_string(),
        };
        match self.entry(name).map_err(read)?.get_password() {
            Ok(key) => Ok(Some(key)),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(error) => Err(read(error)),
        }
    }

    fn set(&self, name: &str, key: &str) -> Result<(), SecretError> {
        let write = |error: KeyringError| SecretError::Write {
            name: name.to_string(),
            reason: error.to_string(),
        };
        self.entry(name)
            .map_err(write)?
            .set_password(key)
            .map_err(write)
    }

    fn delete(&self, name: &str) -> Result<(), SecretError> {
        let write = |error: KeyringError| SecretError::Write {
            name: name.to_string(),
            reason: error.to_string(),
        };
        match self.entry(name).map_err(write)?.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(error) => Err(write(error)),
        }
    }
}

/// Хранилище ОС, которое открывается при первом обращении: воркер без внешних провайдеров его не
/// трогает, а ошибка открытия видна только там, где ключ действительно нужен.
#[derive(Default)]
pub struct OsSecrets {
    inner: std::sync::OnceLock<Result<KeyringSecrets, String>>,
}

impl OsSecrets {
    fn open(&self) -> Result<&KeyringSecrets, SecretError> {
        self.inner
            .get_or_init(|| KeyringSecrets::os().map_err(|error| error.to_string()))
            .as_ref()
            .map_err(|reason| SecretError::Store(reason.clone()))
    }
}

impl SecretStore for OsSecrets {
    fn get(&self, name: &str) -> Result<Option<String>, SecretError> {
        self.open()?.get(name)
    }

    fn set(&self, name: &str, key: &str) -> Result<(), SecretError> {
        self.open()?.set(name, key)
    }

    fn delete(&self, name: &str) -> Result<(), SecretError> {
        self.open()?.delete(name)
    }
}

/// Ключи в памяти процесса, для тестов: настоящее хранилище ОС они не трогают.
#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub struct MemorySecrets {
    keys: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

#[cfg(any(test, feature = "test-support"))]
impl SecretStore for MemorySecrets {
    fn get(&self, name: &str) -> Result<Option<String>, SecretError> {
        Ok(self.keys.lock().unwrap().get(name).cloned())
    }

    fn set(&self, name: &str, key: &str) -> Result<(), SecretError> {
        self.keys
            .lock()
            .unwrap()
            .insert(name.to_string(), key.to_string());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), SecretError> {
        self.keys.lock().unwrap().remove(name);
        Ok(())
    }
}

/// Откуда провайдеру брать ключ: поля `api_key_name` и `api_key_env` из настроек.
#[derive(Clone)]
pub struct KeyRef {
    pub name: Option<String>,
    pub env: Option<String>,
    pub store: Arc<dyn SecretStore>,
}

impl KeyRef {
    /// Ключ на сейчас. Переменная окружения, если она задана и не пуста, перекрывает хранилище.
    /// Ключ читается при каждом запросе: исправленный в хранилище ключ действует без перезапуска.
    pub fn resolve(&self) -> Result<Option<String>, SecretError> {
        if let Some(var) = &self.env
            && let Ok(value) = std::env::var(var)
            && !value.is_empty()
        {
            return Ok(Some(value));
        }
        match &self.name {
            Some(name) => self.store.get(name),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secrets() -> KeyringSecrets {
        KeyringSecrets::with_store(keyring_core::mock::Store::new().unwrap())
    }

    #[test]
    fn key_round_trips_and_a_missing_one_is_none() {
        let secrets = secrets();
        assert_eq!(secrets.get("bars/test").unwrap(), None);
        secrets.set("bars/test", "sk-секрет").unwrap();
        assert_eq!(
            secrets.get("bars/test").unwrap().as_deref(),
            Some("sk-секрет")
        );
        secrets.delete("bars/test").unwrap();
        assert_eq!(secrets.get("bars/test").unwrap(), None);
        secrets.delete("bars/test").unwrap();
    }

    #[test]
    fn environment_overrides_the_store_only_when_set() {
        let store = Arc::new(secrets());
        store.set("bars/test", "из-хранилища").unwrap();
        let var = "BARS_TEST_KEY_OVERRIDE_7F3A";
        let key = KeyRef {
            name: Some("bars/test".into()),
            env: Some(var.into()),
            store,
        };
        assert_eq!(key.resolve().unwrap().as_deref(), Some("из-хранилища"));
        // SAFETY: имя переменной уникально для этого теста, другие потоки её не читают.
        unsafe { std::env::set_var(var, "из-окружения") };
        assert_eq!(key.resolve().unwrap().as_deref(), Some("из-окружения"));
        // SAFETY: то же.
        unsafe { std::env::remove_var(var) };
        assert_eq!(key.resolve().unwrap().as_deref(), Some("из-хранилища"));
    }

    #[test]
    fn no_name_and_no_env_means_no_key() {
        let key = KeyRef {
            name: None,
            env: None,
            store: Arc::new(secrets()),
        };
        assert_eq!(key.resolve().unwrap(), None);
    }
}
