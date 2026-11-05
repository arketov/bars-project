//! Запуск воркера по требованию (`docs/architecture/ipc.md`, «Запуск воркера по требованию»).

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use crate::client::{Client, ClientError};
use crate::endpoint::Endpoint;
use crate::methods::ClientKind;

/// Сколько клиент ждёт воркера после запуска.
pub const LAUNCH_DEADLINE: Duration = Duration::from_secs(15);

/// Файл-метка рядом с exe: установщик кладёт её на время замены файлов
/// (`crates/bars-app/installer-hooks.nsh`), пока она свежая, клиенты не запускают воркер.
pub const UPDATE_MARKER: &str = "update.lock";

/// Через сколько метка считается забытой (установщик упал): без этого клиенты не запускали бы воркер.
pub const UPDATE_MARKER_TTL: Duration = Duration::from_secs(10 * 60);

/// Идентификатор приложения для LaunchServices на macOS (`tauri.conf.json`).
#[cfg(target_os = "macos")]
const BUNDLE_ID: &str = "io.github.arketov.bars";

/// Идёт ли установка или удаление в папке `dir`: рядом лежит свежая метка установщика.
/// Нет метки, она не читается или старше [`UPDATE_MARKER_TTL`] — нет.
pub fn update_in_progress(dir: &Path) -> bool {
    update_in_progress_at(dir, SystemTime::now())
}

fn update_in_progress_at(dir: &Path, now: SystemTime) -> bool {
    let Ok(modified) = std::fs::metadata(dir.join(UPDATE_MARKER)).and_then(|meta| meta.modified())
    else {
        return false;
    };
    // Время метки впереди часов (сдвиг часов): терпим небольшой запас, но не вечную метку.
    let age = match now.duration_since(modified) {
        Ok(age) => age,
        Err(ahead) => ahead.duration(),
    };
    age < UPDATE_MARKER_TTL
}

/// `bars` рядом с исполняемым файлом клиента: все три exe поставляются вместе.
pub fn worker_exe() -> io::Result<PathBuf> {
    let me = std::env::current_exe()?;
    let dir = me
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "у exe клиента нет папки"))?;
    Ok(dir.join(if cfg!(windows) { "bars.exe" } else { "bars" }))
}

/// Подключается к воркеру; если он не запущен — запускает `bars --background` и повторяет
/// подключение до [`LAUNCH_DEADLINE`]. Не запускает воркер, если канал занят чужим процессом.
pub async fn connect_or_launch(
    endpoint: &Endpoint,
    kind: ClientKind,
) -> Result<Client, ClientError> {
    connect_or_launch_with(endpoint, kind, LAUNCH_DEADLINE, || {
        spawn_worker(&worker_exe()?)
    })
    .await
}

/// То же с заданным способом запуска и сроком: для тестов и клиентов со своим `bars`.
pub async fn connect_or_launch_with(
    endpoint: &Endpoint,
    kind: ClientKind,
    deadline: Duration,
    launch: impl FnOnce() -> io::Result<()>,
) -> Result<Client, ClientError> {
    match Client::connect(endpoint, kind).await {
        Err(ClientError::Unavailable(_)) => {}
        other => return other,
    }
    launch().map_err(|error| {
        ClientError::Unavailable(io::Error::new(
            error.kind(),
            format!("не удалось запустить bars --background: {error}"),
        ))
    })?;
    Client::connect_with_retry(endpoint, kind, deadline)
        .await
        .map_err(|error| match error {
            ClientError::Unavailable(error) => ClientError::Unavailable(io::Error::new(
                error.kind(),
                format!(
                    "воркер не ответил за {} с после запуска ({error}); логи — ~/.bars/logs",
                    deadline.as_secs()
                ),
            )),
            other => other,
        })
}

/// Запускает `bars --background` отвязанным процессом: воркер переживает клиента и сессию агента.
/// Во время установки (свежая метка [`UPDATE_MARKER`] рядом с `exe`) не запускает: иначе прежний
/// воркер занял бы файлы, которые установщик собрался заменить (ISS-107); возвращает ошибку,
/// которую агент читает как просьбу повторить позже.
pub fn spawn_worker(exe: &Path) -> io::Result<()> {
    if exe.parent().is_some_and(update_in_progress) {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "идёт обновление bars: повторите запрос через минуту",
        ));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        let start = |flags: u32| {
            Command::new(exe)
                .arg("--background")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(flags)
                .spawn()
        };
        let base = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
        // Job object клиента может запрещать отвязку: тогда без неё.
        start(base | CREATE_BREAKAWAY_FROM_JOB)
            .or_else(|_| start(base))
            .map(drop)
    }
    #[cfg(target_os = "macos")]
    {
        let _ = exe;
        Command::new("open")
            .args(["-b", BUNDLE_ID, "--args", "--background"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(drop)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new(exe)
            .arg("--background")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Пустая папка под метку; удаляется при выходе из теста.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("bars-launch-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn marker_modified(&self, at: SystemTime) {
            let file = std::fs::File::create(self.0.join(UPDATE_MARKER)).unwrap();
            file.set_modified(at).unwrap();
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn no_marker_means_no_update() {
        let dir = TempDir::new("none");
        assert!(!update_in_progress(&dir.0));
    }

    #[test]
    fn fresh_marker_means_update_in_progress() {
        let dir = TempDir::new("fresh");
        dir.marker_modified(SystemTime::now());
        assert!(update_in_progress(&dir.0));
    }

    #[test]
    fn forgotten_marker_expires() {
        let dir = TempDir::new("stale");
        let long_ago = SystemTime::now() - UPDATE_MARKER_TTL - Duration::from_secs(60);
        dir.marker_modified(long_ago);
        assert!(!update_in_progress(&dir.0));
    }

    #[test]
    fn marker_slightly_ahead_of_clock_still_counts_but_far_ahead_does_not() {
        let dir = TempDir::new("ahead");
        dir.marker_modified(SystemTime::now() + Duration::from_secs(30));
        assert!(update_in_progress(&dir.0));
        dir.marker_modified(SystemTime::now() + UPDATE_MARKER_TTL + Duration::from_secs(3600));
        assert!(!update_in_progress(&dir.0));
    }

    #[test]
    fn spawn_worker_refuses_during_update_without_starting_anything() {
        let dir = TempDir::new("spawn");
        dir.marker_modified(SystemTime::now());
        // Файла bars рядом нет: без метки была бы ошибка запуска `NotFound`, с меткой — отказ раньше.
        let error = spawn_worker(&dir.0.join("bars.exe")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        assert!(error.to_string().contains("обновление"));
    }

    #[tokio::test]
    async fn launch_failure_is_reported_as_unavailable() {
        let endpoint = Endpoint::named("launch-fail");
        let error = connect_or_launch_with(
            &endpoint,
            ClientKind::Cli,
            Duration::from_millis(100),
            || Err(io::Error::new(io::ErrorKind::NotFound, "нет bars")),
        )
        .await
        .err()
        .unwrap();
        assert!(
            matches!(&error, ClientError::Unavailable(e) if e.to_string().contains("нет bars"))
        );
        assert_eq!(error.exit_code(), crate::EXIT_WORKER_UNAVAILABLE);
    }

    #[tokio::test]
    async fn worker_that_never_comes_up_times_out_with_a_hint() {
        let endpoint = Endpoint::named("launch-timeout");
        let launched = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&launched);
        let error = connect_or_launch_with(
            &endpoint,
            ClientKind::Cli,
            Duration::from_millis(200),
            move || {
                flag.store(true, Ordering::SeqCst);
                Ok(())
            },
        )
        .await
        .err()
        .unwrap();
        assert!(launched.load(Ordering::SeqCst));
        assert!(matches!(&error, ClientError::Unavailable(e) if e.to_string().contains("логи")));
    }
}
