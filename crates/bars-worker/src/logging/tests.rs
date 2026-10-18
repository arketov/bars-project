use super::*;

fn entry(level: Level, n: usize) -> Entry {
    Entry {
        time: "2026-10-18T12:00:00Z".into(),
        pid: 1,
        level,
        target: "bars_worker".into(),
        message: format!("Событие {n}"),
    }
}

#[test]
fn rotate_restart_filter_and_bound() {
    let temp = tempfile::tempdir().unwrap();
    let journal = Journal::open(temp.path(), 1024).unwrap();
    for n in 0..100 {
        journal
            .append(&entry(
                if n % 2 == 0 {
                    Level::Info
                } else {
                    Level::Error
                },
                n,
            ))
            .unwrap();
    }
    for n in 0..=ARCHIVES {
        assert!(fs::metadata(journal.file(n)).unwrap().len() <= 1024);
    }
    assert_eq!(fs::read_dir(&journal.directory).unwrap().count(), 4);
    let before = journal.snapshot(Level::Error).unwrap();
    assert_eq!(before.entries[0].message, "Событие 99");
    assert!(before.entries.iter().all(|e| e.level == Level::Error));
    assert!(!before.entries.iter().any(|e| e.message == "Событие 1"));
    let reopened = Journal::open(temp.path(), 1024).unwrap();
    reopened.append(&entry(Level::Warn, 100)).unwrap();
    assert_eq!(
        reopened.snapshot(Level::Warn).unwrap().entries[0].message,
        "Событие 100"
    );
}

#[test]
fn concurrent_read_write_and_corruption() {
    let temp = tempfile::tempdir().unwrap();
    let journal = Journal::open(temp.path(), MAX_BYTES).unwrap();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let j = &journal;
            scope.spawn(move || {
                for n in 0..200 {
                    j.append(&entry(Level::Debug, n)).unwrap();
                }
            });
        }
        let j = &journal;
        scope.spawn(move || {
            for _ in 0..40 {
                assert_eq!(j.snapshot(Level::Debug).unwrap().skipped, 0);
            }
        });
    });
    assert_eq!(journal.snapshot(Level::Debug).unwrap().entries.len(), LIMIT);
    OpenOptions::new()
        .append(true)
        .open(journal.file(0))
        .unwrap()
        .write_all(b"bad-json\n")
        .unwrap();
    assert_eq!(journal.snapshot(Level::Debug).unwrap().skipped, 1);
    assert!(journal.snapshot(Level::Error).unwrap().entries.is_empty());
}

#[test]
fn write_failure_is_visible_and_recovers() {
    let temp = tempfile::tempdir().unwrap();
    let journal = Journal::open(temp.path(), MAX_BYTES).unwrap();
    fs::remove_file(journal.file(0)).unwrap();
    fs::create_dir(journal.file(0)).unwrap();
    assert!(journal.append(&entry(Level::Info, 0)).is_err());
    fs::remove_dir(journal.file(0)).unwrap();
    assert!(journal.snapshot(Level::Info).unwrap().write_error);
    journal.append(&entry(Level::Info, 1)).unwrap();
    assert!(!journal.snapshot(Level::Info).unwrap().write_error);
}

#[test]
fn restart_after_partial_line_preserves_next_event() {
    let temp = tempfile::tempdir().unwrap();
    let journal = Journal::open(temp.path(), MAX_BYTES).unwrap();
    fs::write(journal.file(0), b"{unfinished").unwrap();
    let reopened = Journal::open(temp.path(), MAX_BYTES).unwrap();
    reopened.append(&entry(Level::Info, 2)).unwrap();
    let snapshot = reopened.snapshot(Level::Debug).unwrap();
    assert_eq!(snapshot.skipped, 1);
    assert_eq!(snapshot.entries[0].message, "Событие 2");
}

#[test]
fn only_bars_targets_and_supported_levels() {
    use log::Log;
    for target in ["ureq", "hf_hub", "ort", "bars_foreign"] {
        assert!(
            !LOGGER.enabled(
                &log::Metadata::builder()
                    .target(target)
                    .level(log::Level::Error)
                    .build()
            )
        );
    }
    assert!(
        LOGGER.enabled(
            &log::Metadata::builder()
                .target("bars_core::models")
                .level(log::Level::Debug)
                .build()
        )
    );
    assert!(
        !LOGGER.enabled(
            &log::Metadata::builder()
                .target("bars_worker")
                .level(log::Level::Trace)
                .build()
        )
    );
}

#[test]
fn process_fixture() {
    let Some(path) = std::env::var_os("BARS_LOG_FIXTURE_HOME") else {
        return;
    };
    let path = PathBuf::from(path);
    init(&path).unwrap();
    log::info!("Запуск тестового процесса");
    log::debug!("Отладочное событие");
    log::error!(target: "ureq", "secret-provider-response");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        use crate::worker::{Worker, WorkerConfig, models::fake::FakeProvider};
        let project = path.join("project");
        fs::create_dir_all(project.join("docs")).unwrap();
        fs::write(
            project.join("docs/test.md"),
            "---\ntitle: Проверка\n---\n# Проверка\nsecret-document-content",
        )
        .unwrap();
        let worker = Worker::start(
            WorkerConfig::new(path.join("profile")),
            FakeProvider::new("m1"),
        )
        .unwrap();
        worker.register(&project).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while !worker.is_idle() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let handler = crate::handler::WorkerHandler::new(worker.clone());
        let call: bars_protocol::SearchParams = serde_json::from_value(serde_json::json!({
            "cwd": path.join("missing").display().to_string(), "query": "secret-query-content"
        }))
        .unwrap();
        assert!(
            handler
                .dispatch(bars_protocol::Call::Search(call))
                .await
                .is_err()
        );
        worker.shutdown().await;
    });
    log::info!("Остановка тестового процесса");
}

#[test]
fn two_processes_keep_history_without_payloads() {
    let temp = tempfile::tempdir().unwrap();
    for _ in 0..2 {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "logging::tests::process_fixture", "--nocapture"])
            .env("BARS_LOG_FIXTURE_HOME", temp.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let journal = Journal::open(temp.path(), MAX_BYTES).unwrap();
    let snapshot = journal.snapshot(Level::Debug).unwrap();
    assert_eq!(snapshot.skipped, 0);
    assert!(
        snapshot
            .entries
            .iter()
            .any(|e| e.message == "Воркер остановлен")
    );
    assert_eq!(
        snapshot
            .entries
            .iter()
            .filter(|e| e.message == "Запуск тестового процесса")
            .count(),
        2
    );
    assert!(
        snapshot
            .entries
            .iter()
            .any(|e| e.message.contains("Индексация завершена"))
    );
    assert!(
        snapshot
            .entries
            .iter()
            .any(|e| e.message.contains("Поиск не выполнен"))
    );
    let text = fs::read_to_string(journal.file(0)).unwrap();
    for secret in [
        "secret-provider-response",
        "secret-document-content",
        "secret-query-content",
    ] {
        assert!(!text.contains(secret));
    }
}
