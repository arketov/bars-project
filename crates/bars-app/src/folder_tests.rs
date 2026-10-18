//! Проверка служб Tauri-команд с настоящим воркером и подделкой модели; без действий ОС.
use std::{fs, path::PathBuf, time::Duration};

use crate::commands::{Shared, execute_folder, prepare_folder};
use bars_worker::{
    handler::WorkerHandler,
    worker::{Worker, WorkerConfig, models::fake::FakeProvider},
};

struct Fixture {
    _temp: tempfile::TempDir,
    worker: Worker,
    shared: Shared,
    project: PathBuf,
}

impl Fixture {
    async fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("Проект с пробелами");
        fs::create_dir_all(project.join("docs/модуль/Папка с пробелами")).unwrap();
        fs::write(
            project.join("docs/модуль/Папка с пробелами/Тема.md"),
            "# Тема\n\nТекст.\n",
        )
        .unwrap();
        let worker = Worker::start(
            WorkerConfig::new(temp.path().join("home")),
            FakeProvider::new("m1"),
        )
        .unwrap();
        worker.register(&project).unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                tokio::time::sleep(Duration::from_millis(20)).await;
                if worker.status(&project).unwrap().updated_at.is_some()
                    && worker.queued().is_empty()
                    && worker.progress().is_none()
                {
                    break;
                }
            }
        })
        .await
        .unwrap();
        worker.pause();
        let shared = Shared::default();
        shared.set(WorkerHandler::new(worker.clone()));
        Self {
            _temp: temp,
            worker,
            shared,
            project,
        }
    }

    async fn pin(&self, path: &str) -> String {
        prepare_folder(
            &self.shared,
            self.project.display().to_string(),
            "docs".into(),
            path.into(),
        )
        .await
        .unwrap()["id"]
            .as_str()
            .unwrap()
            .into()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn folder_command_returns_full_project_receipt_and_validated_path() {
    let f = Fixture::new().await;
    let id = f.pin("модуль/Папка с пробелами").await;
    let path = execute_folder(&f.shared, id.clone(), "path".into())
        .await
        .unwrap();
    assert_eq!(
        PathBuf::from(path["absolute"].as_str().unwrap()),
        f.project
            .join("docs/модуль/Папка с пробелами")
            .canonicalize()
            .unwrap()
    );
    let accepted = execute_folder(&f.shared, id, "check".into()).await.unwrap();
    assert!(
        accepted["operation_id"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            > 0
    );
    let queue = f.worker.queued();
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].kind, bars_worker::worker::JobKind::Check);
    assert_eq!(queue[0].priority, bars_worker::worker::Priority::Explicit);
    f.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn folder_command_rejects_stale_registration_and_docs_root_roundtrip() {
    let f = Fixture::new().await;
    let id = f.pin(".").await;
    f.worker.set_docs_root(&f.project, ".").unwrap();
    f.worker.set_docs_root(&f.project, "docs").unwrap();
    assert!(
        execute_folder(&f.shared, id, "path".into())
            .await
            .unwrap_err()
            .contains("устарела")
    );
    let id = f.pin(".").await;
    f.worker
        .delete_project(&f.project.display().to_string())
        .await
        .unwrap();
    f.worker.register(&f.project).unwrap();
    assert!(
        execute_folder(&f.shared, id, "check".into())
            .await
            .unwrap_err()
            .contains("устарела")
    );
    f.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn folder_command_allows_deleted_folder_path_and_check_but_rejects_open() {
    let f = Fixture::new().await;
    let id = f.pin("модуль/Папка с пробелами").await;
    let folder = f.project.join("docs/модуль/Папка с пробелами");
    fs::remove_file(folder.join("Тема.md")).unwrap();
    fs::remove_dir(&folder).unwrap();
    assert!(
        execute_folder(&f.shared, id.clone(), "path".into())
            .await
            .unwrap()["missing"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(
        execute_folder(&f.shared, id.clone(), "open".into())
            .await
            .unwrap_err(),
        "Папка не найдена"
    );
    assert!(execute_folder(&f.shared, id, "check".into()).await.is_ok());
    f.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn folder_command_rejects_forged_paths_and_unknown_actions() {
    let f = Fixture::new().await;
    for path in ["../docs", "C:/Windows", "моду", "неизвестная"] {
        assert!(
            prepare_folder(
                &f.shared,
                f.project.display().to_string(),
                "docs".into(),
                path.into()
            )
            .await
            .is_err()
        );
    }
    assert!(
        prepare_folder(
            &f.shared,
            f.project.display().to_string(),
            ".".into(),
            ".".into()
        )
        .await
        .is_err()
    );
    assert!(
        execute_folder(&f.shared, "999999".into(), "path".into())
            .await
            .is_err()
    );
    let id = f.pin(".").await;
    assert!(
        execute_folder(&f.shared, id, "unknown".into())
            .await
            .is_err()
    );
    f.worker.shutdown().await;
}
