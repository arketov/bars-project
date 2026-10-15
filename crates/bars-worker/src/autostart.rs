//! Автозапуск `bars` при входе в систему (`docs/features/distribution/autostart.md`).
//!
//! Состояние живёт в ОС, а не в `settings.toml`: на Windows это значение `bars` в
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. Воркер читает и пишет его через
//! [`Autostart`]; тесты и другие ОС подставляют своё хранилище.

use std::io;
use std::sync::Mutex;

/// Состояние автозапуска в ОС.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutostartState {
    /// ОС и сборка умеют автозапуск.
    pub supported: bool,
    /// Запись есть, и ОС её не отключила (Windows: «Диспетчер задач → Автозагрузка»).
    pub enabled: bool,
    /// Запись есть, но указывает не на текущий `bars`: приложение переехало.
    pub stale: bool,
    /// Команда из записи, если она есть.
    pub command: Option<String>,
}

impl AutostartState {
    pub fn unsupported() -> Self {
        Self {
            supported: false,
            enabled: false,
            stale: false,
            command: None,
        }
    }
}

/// Хранилище записи автозапуска.
pub trait Autostart: Send + Sync {
    fn state(&self) -> io::Result<AutostartState>;
    /// Включает или выключает автозапуск и возвращает новое состояние. Повтор безвреден.
    fn set(&self, enabled: bool) -> io::Result<AutostartState>;
}

/// Автозапуска нет: ОС без реализации и тесты, которым реестр трогать нельзя.
#[derive(Debug, Default)]
pub struct NoAutostart;

impl Autostart for NoAutostart {
    fn state(&self) -> io::Result<AutostartState> {
        Ok(AutostartState::unsupported())
    }

    fn set(&self, _enabled: bool) -> io::Result<AutostartState> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "автозапуск на этой ОС не поддерживается",
        ))
    }
}

/// Запись в памяти: для тестов обработчика.
#[derive(Debug)]
pub struct MemoryAutostart {
    command: Mutex<Option<String>>,
    wanted: String,
}

impl MemoryAutostart {
    /// `wanted` — команда, которую запись должна содержать; `stored` — что лежит в ней сейчас.
    pub fn new(wanted: &str, stored: Option<&str>) -> Self {
        Self {
            command: Mutex::new(stored.map(str::to_string)),
            wanted: wanted.to_string(),
        }
    }
}

impl Autostart for MemoryAutostart {
    fn state(&self) -> io::Result<AutostartState> {
        let command = crate::worker::lock(&self.command).clone();
        Ok(AutostartState {
            supported: true,
            enabled: command.is_some(),
            stale: command
                .as_ref()
                .is_some_and(|stored| *stored != self.wanted),
            command,
        })
    }

    fn set(&self, enabled: bool) -> io::Result<AutostartState> {
        *crate::worker::lock(&self.command) = enabled.then(|| self.wanted.clone());
        self.state()
    }
}

/// Команда автозапуска: окно скрыто, `bars` ждёт в трее (`--background`, как у клиентов).
pub fn command_for(exe: &std::path::Path) -> String {
    format!("\"{}\" --background", exe.display())
}

/// Если запись есть, но указывает на другое расположение `bars`, переписывает её на текущее.
/// Возвращает `true`, если запись исправлена. Выключенный автозапуск не включается.
pub fn repair(autostart: &dyn Autostart) -> io::Result<bool> {
    let state = autostart.state()?;
    if state.supported && state.stale {
        autostart.set(true)?;
        return Ok(true);
    }
    Ok(false)
}

#[cfg(windows)]
mod run_key;
#[cfg(windows)]
pub use run_key::RunKey;

/// Автозапуск текущей ОС для `bars` по пути `exe`.
#[cfg(windows)]
pub fn for_exe(exe: &std::path::Path) -> std::sync::Arc<dyn Autostart> {
    std::sync::Arc::new(RunKey::new(command_for(exe)))
}

/// Автозапуска на этой ОС пока нет (macOS — LaunchAgent, когда появится Mac).
#[cfg(not(windows))]
pub fn for_exe(_exe: &std::path::Path) -> std::sync::Arc<dyn Autostart> {
    std::sync::Arc::new(NoAutostart)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_rewrites_a_stale_entry_but_not_a_missing_one() {
        let moved = MemoryAutostart::new("\"new\" --background", Some("\"old\" --background"));
        assert!(repair(&moved).unwrap());
        let state = moved.state().unwrap();
        assert!(state.enabled && !state.stale);

        let off = MemoryAutostart::new("\"new\" --background", None);
        assert!(!repair(&off).unwrap());
        assert!(!off.state().unwrap().enabled);

        assert!(!repair(&NoAutostart).unwrap());
    }

    #[test]
    fn set_is_idempotent() {
        let store = MemoryAutostart::new("c", None);
        assert!(store.set(true).unwrap().enabled);
        assert!(store.set(true).unwrap().enabled);
        assert!(!store.set(false).unwrap().enabled);
        assert!(!store.set(false).unwrap().enabled);
    }
}
