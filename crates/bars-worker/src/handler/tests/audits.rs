//! Аудиты знаний: запуск, очередь, сравнение запусков, расписание и удаление проекта
//! (`docs/features/knowledge-base/audits.md`, A-01…A-07). Окно вызывает обработчик напрямую.

use bars_core::secrets::MemorySecrets;
use bars_protocol::{
    AuditCancelParams, AuditFindingsOutput, AuditFindingsParams, AuditListOutput, AuditListParams,
    AuditRunOutput, AuditRunParams, AuditScheduleParams, AuditSummary, ProjectDeleteParams,
};

use super::*;
use crate::autostart::MemoryAutostart;
use crate::handler::WorkerHandler;
use crate::worker::Worker;

struct Env {
    projects: TempDir,
    worker: Worker,
    handler: WorkerHandler,
    _home: TempDir,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn audit_scheduler_is_joined_by_worker_shutdown() {
    let env = Env::new();
    env.handler.start_audit_scheduler();
    tokio::time::timeout(Duration::from_secs(5), env.worker.shutdown())
        .await
        .unwrap();
    assert!(env.worker.is_stopping());
    env.handler.audit_schedule_tick().await;
}

impl Env {
    fn new() -> Self {
        let home = TempDir::new().unwrap();
        let mut config = WorkerConfig::new(home.path().to_path_buf()).without_graph_pause();
        config.secrets = Arc::new(MemorySecrets::default());
        config.autostart = Arc::new(MemoryAutostart::new("\"/opt/bars/bars\"", None));
        let worker = Worker::start(config, FakeProvider::new("m1")).unwrap();
        let handler = WorkerHandler::new(worker.clone());
        Self {
            projects: TempDir::new().unwrap(),
            worker,
            handler,
            _home: home,
        }
    }

    /// Проект с документами; ждёт, пока индекс готов.
    async fn project(&self, files: &[(&str, &str)]) -> PathBuf {
        let root = self.projects.path().join("proj");
        fs::create_dir_all(root.join("docs")).unwrap();
        for (name, text) in files {
            let path = root.join("docs").join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        self.worker.register(&root).unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            tokio::time::sleep(Duration::from_millis(60)).await;
            let status = self.worker.status(&root).unwrap();
            if self.worker.is_idle() && status.state == ProjectState::Ready && status.pending == 0 {
                return root;
            }
            assert!(Instant::now() < deadline, "индекс не готов");
        }
    }

    async fn call<T: serde::de::DeserializeOwned>(&self, call: Call) -> T {
        serde_json::from_value(self.handler.dispatch(call).await.unwrap()).unwrap()
    }

    async fn list(&self, root: &Path) -> AuditListOutput {
        self.call(Call::AuditList(AuditListParams {
            project: root.display().to_string(),
        }))
        .await
    }

    async fn summary(&self, root: &Path, kind: &str) -> AuditSummary {
        self.list(root)
            .await
            .audits
            .into_iter()
            .find(|a| a.kind == kind)
            .unwrap()
    }

    async fn run(&self, root: &Path, kind: &str) -> AuditRunOutput {
        self.call(Call::AuditRun(AuditRunParams {
            project: root.display().to_string(),
            kind: kind.into(),
        }))
        .await
    }

    /// Ждёт, пока вид перестанет идти или ждать очереди.
    async fn finished(&self, root: &Path, kind: &str) -> AuditSummary {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let summary = self.summary(root, kind).await;
            if summary.state != "running"
                && summary.state != "queued"
                && summary.last_run_at.is_some()
            {
                return summary;
            }
            assert!(
                Instant::now() < deadline,
                "аудит не завершился: {summary:?}"
            );
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }

    async fn findings(&self, root: &Path, kind: &str, group: Option<&str>) -> AuditFindingsOutput {
        self.call(Call::AuditFindings(AuditFindingsParams {
            project: root.display().to_string(),
            kind: kind.into(),
            group: group.map(str::to_string),
            include_gone: true,
            include_ignored: false,
            cursor: None,
            limit: None,
        }))
        .await
    }
}

fn md_hashes(root: &Path) -> Vec<(PathBuf, [u8; 32])> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n == ".bars") {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "md") {
                found.push((
                    path.clone(),
                    *blake3::hash(&fs::read(&path).unwrap()).as_bytes(),
                ));
            }
        }
    }
    found.sort();
    found
}

const PARAGRAPH: &str = "Воркер держит очередь документов и по очереди отдаёт их индексатору, пока \
    модель не загружена, а пользователь может в это время искать по уже готовым векторам и текстам, \
    не дожидаясь окончания индексации всего проекта целиком.";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn contradictions_and_ignore_round_trip_without_changing_files() {
    let env = Env::new();
    let root = env.project(&[
        ("features/a.md", "# Настройки\n\nВремя ожидания ответа сервера в текущих настройках приложения составляет 30 с. Это значение задано по умолчанию для всех проектов.\n"),
        ("manual/b.md", "# Настройки\n\nВремя ожидания ответа сервера в текущих настройках приложения составляет 60 секунд. Это значение задано по умолчанию для всех проектов.\n"),
        ("chats/old.md", "# Настройки\n\nВремя ожидания ответа сервера в текущих настройках приложения составляет 90 с. Это значение задано по умолчанию для всех проектов.\n"),
    ]).await;
    let before = md_hashes(&root);
    env.run(&root, "contradictions").await;
    let summary = env.finished(&root, "contradictions").await;
    assert_eq!(summary.state, "complete");
    assert_eq!(summary.total, 1);
    let page = env.findings(&root, "contradictions", None).await;
    let finding = &page.items[0];
    assert!(finding.reason.contains("Возможное расхождение"));
    assert!(finding.subjects.iter().all(|s| !s.path.contains("chats/")));
    let _: bars_protocol::AuditIgnoreOutput = env
        .call(Call::AuditIgnore(bars_protocol::AuditIgnoreParams {
            project: root.display().to_string(),
            kind: "contradictions".into(),
            fingerprint: finding.fingerprint.clone(),
            ignored: true,
        }))
        .await;
    assert_eq!(env.summary(&root, "contradictions").await.total, 0);
    assert!(
        env.findings(&root, "contradictions", None)
            .await
            .items
            .is_empty()
    );
    let all: AuditFindingsOutput = env
        .call(Call::AuditFindings(AuditFindingsParams {
            project: root.display().to_string(),
            kind: "contradictions".into(),
            group: None,
            include_gone: true,
            include_ignored: true,
            cursor: None,
            limit: None,
        }))
        .await;
    assert_eq!(all.total, 1);
    assert!(all.items[0].ignored);
    let _: bars_protocol::AuditIgnoreOutput = env
        .call(Call::AuditIgnore(bars_protocol::AuditIgnoreParams {
            project: root.display().to_string(),
            kind: "contradictions".into(),
            fingerprint: finding.fingerprint.clone(),
            ignored: false,
        }))
        .await;
    assert_eq!(env.summary(&root, "contradictions").await.total, 1);
    assert_eq!(md_hashes(&root), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn broken_links_run_compares_runs_and_never_touches_markdown() {
    let env = Env::new();
    let root = env
        .project(&[
            ("a.md", "# Альфа\n\n[бета](b.md) и [[нет]]\n"),
            ("b.md", "# Бета\n\n[гамма](g.md)\n"),
        ])
        .await;
    let before = md_hashes(&root);

    assert_eq!(env.summary(&root, "broken_links").await.state, "never");
    let started = env.run(&root, "broken_links").await;
    assert!(started.accepted && !started.already);
    let first = env.finished(&root, "broken_links").await;
    assert_eq!(
        (first.state.as_str(), first.total, first.new_count),
        ("complete", 2, 0)
    );
    // A-01: аудит читает документацию и ничего в ней не меняет.
    assert_eq!(md_hashes(&root), before);
    let page = env.findings(&root, "broken_links", None).await;
    assert_eq!(page.total, 2);
    let subjects: Vec<_> = page
        .items
        .iter()
        .map(|f| (f.subjects[0].path.as_str(), f.subjects[0].first_line))
        .collect();
    assert_eq!(subjects, vec![("docs/a.md", 3), ("docs/b.md", 3)]);
    assert!(
        page.items
            .iter()
            .all(|f| f.standing == "known" && f.tag == "broken")
    );

    // Повторный запуск: одна находка исчезла, одна появилась, одна прежняя.
    fs::write(root.join("docs/g.md"), "# Гамма\n").unwrap();
    fs::write(root.join("docs/b.md"), "# Бета\n\n[дельта](d.md)\n").unwrap();
    env.run(&root, "broken_links").await;
    let second = loop {
        let summary = env.finished(&root, "broken_links").await;
        if summary.result_run_id != first.result_run_id {
            break summary;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    };
    assert_eq!(
        (second.total, second.new_count, second.gone_count),
        (2, 1, 1)
    );
    let page = env.findings(&root, "broken_links", None).await;
    let standings: Vec<_> = page.items.iter().map(|f| f.standing.as_str()).collect();
    assert_eq!(standings, vec!["known", "new", "gone"]);

    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicates_run_splits_main_and_overlap_and_names_document_roles() {
    let env = Env::new();
    let root = env
        .project(&[
            (
                "manual/one.md",
                &format!("# Один\n\n## Очередь\n\n{PARAGRAPH}\n"),
            ),
            (
                "features/two.md",
                &format!("# Два\n\n## Другая очередь\n\n{PARAGRAPH}\n"),
            ),
            // Два раздела одного документа с одним текстом не считаются повтором частичного вида,
            // но точная группа остаётся точной.
            ("notes.md", "# Заметка\n\nКороткий текст.\n"),
        ])
        .await;

    env.run(&root, "duplicates").await;
    let done = env.finished(&root, "duplicates").await;
    assert_eq!(done.state, "complete", "{done:?}");
    let main = env.findings(&root, "duplicates", Some("main")).await;
    let exact = main
        .items
        .iter()
        .find(|f| f.tag == "exact")
        .unwrap_or_else(|| panic!("точная находка не найдена: {main:?}"));
    let roles: Vec<_> = exact.subjects.iter().map(|s| s.role.as_str()).collect();
    assert!(
        roles.contains(&"manual") && roles.contains(&"feature"),
        "{roles:?}"
    );
    assert!(exact.detail.contains("Воркер держит очередь"));
    assert_eq!(done.total, main.total);
    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn running_audit_is_returned_not_duplicated_and_other_kind_waits() {
    let env = Env::new();
    let root = env
        .project(&[("a.md", &format!("# Альфа\n\n{PARAGRAPH}\n"))])
        .await;
    // Замок записи проекта держит запуск `duplicates` на чтении, пока мы его не отпустим.
    let gate = env.worker.gate(&root).unwrap().write_owned().await;
    let first = env.run(&root, "duplicates").await;
    assert!(first.accepted && !first.already);
    assert_eq!(env.summary(&root, "duplicates").await.state, "running");
    let again = env.run(&root, "duplicates").await;
    assert!(again.already);
    let other = env.run(&root, "broken_links").await;
    assert!(other.queued && !other.already);
    assert_eq!(env.summary(&root, "broken_links").await.state, "queued");
    assert!(
        env.call::<bars_protocol::AuditCancelOutput>(Call::AuditCancel(AuditCancelParams {
            project: root.display().to_string(),
            kind: "broken_links".into(),
        }))
        .await
        .cancelled
    );
    drop(gate);
    env.finished(&root, "duplicates").await;
    assert_eq!(env.summary(&root, "broken_links").await.state, "never");
    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn schedule_validates_modes_and_on_change_runs_only_after_changes() {
    let env = Env::new();
    let root = env.project(&[("a.md", "# Альфа\n\n[[нет]]\n")]).await;
    let project = root.display().to_string();
    let set = |kind: &str, mode: &str| {
        Call::AuditSchedule(AuditScheduleParams {
            project: project.clone(),
            kind: kind.into(),
            mode: mode.into(),
        })
    };
    // «При изменении» только у битых ссылок.
    assert!(
        env.handler
            .dispatch(set("duplicates", "on_change"))
            .await
            .is_err()
    );
    assert!(
        env.handler
            .dispatch(set("broken_links", "hourly"))
            .await
            .is_err()
    );
    env.handler
        .dispatch(set("duplicates", "weekly"))
        .await
        .unwrap();
    assert_eq!(env.summary(&root, "duplicates").await.schedule, "weekly");
    assert_eq!(env.summary(&root, "duplicates").await.state, "never");
    assert_eq!(
        env.summary(&root, "broken_links").await.schedule,
        "on_change"
    );

    env.handler
        .dispatch(set("broken_links", "on_change"))
        .await
        .unwrap();
    env.handler.audit_schedule_tick().await;
    let first = env.finished(&root, "broken_links").await;
    assert_eq!(first.total, 1);
    // Без правок в документации следующая проверка вид не запускает.
    env.handler.audit_schedule_tick().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let same = env.summary(&root, "broken_links").await;
    assert_eq!(same.result_run_id, first.result_run_id);
    // После правки — запускает.
    fs::write(root.join("docs/a.md"), "# Альфа\n\n[[нет]] и [[ещё нет]]\n").unwrap();
    env.handler.audit_schedule_tick().await;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let now = env.summary(&root, "broken_links").await;
        if now.result_run_id != first.result_run_id && now.state == "complete" {
            assert_eq!(now.total, 2);
            break;
        }
        assert!(Instant::now() < deadline, "перезапуск не случился: {now:?}");
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    // Расписание off ничего не запускает.
    env.handler
        .dispatch(set("broken_links", "off"))
        .await
        .unwrap();
    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn window_methods_reject_relative_project_and_unknown_kind() {
    let env = Env::new();
    let root = env.project(&[("a.md", "# Альфа\n")]).await;
    let error = env
        .handler
        .dispatch(Call::AuditList(AuditListParams {
            project: "proj".into(),
        }))
        .await
        .unwrap_err();
    assert!(error.message.contains("абсолютный путь"));
    let error = env
        .handler
        .dispatch(Call::AuditRun(AuditRunParams {
            project: root.display().to_string(),
            kind: "unsupported_audit".into(),
        }))
        .await
        .unwrap_err();
    assert!(error.message.contains("неизвестный вид"));
    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deleting_a_project_removes_the_audit_store_and_keeps_foreign_files() {
    let env = Env::new();
    let root = env.project(&[("a.md", "# Альфа\n\n[[нет]]\n")]).await;
    env.run(&root, "broken_links").await;
    env.finished(&root, "broken_links").await;
    let store = root.join(".bars/audits.sqlite3");
    assert!(store.is_file());
    fs::write(root.join(".bars/foreign.txt"), "чужой файл").unwrap();
    env.handler
        .dispatch(Call::ProjectDelete(ProjectDeleteParams {
            project: root.display().to_string(),
            confirmed: true,
        }))
        .await
        .unwrap();
    assert!(!store.exists());
    assert!(root.join(".bars/foreign.txt").is_file());
    env.worker.shutdown().await;
}

/// ISS-106: проект без папки корня документации расписание пропускает, пометка о пропуске ставится
/// один раз и снимается, когда папка вернулась.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn schedule_skips_project_without_docs_root_and_notes_it_once() {
    let env = Env::new();
    let root = env.project(&[("a.md", "# А\n\nТекст про аудиты.\n")]).await;
    assert!(env.handler.schedule_docs_root_present(&root, Some("docs")));
    assert!(env.handler.schedule_blocked.lock().unwrap().is_empty());

    assert!(
        !env.handler
            .schedule_docs_root_present(&root, Some("нет-такой"))
    );
    assert!(
        !env.handler
            .schedule_docs_root_present(&root, Some("нет-такой"))
    );
    assert_eq!(env.handler.schedule_blocked.lock().unwrap().len(), 1);

    assert!(env.handler.schedule_docs_root_present(&root, Some("docs")));
    assert!(env.handler.schedule_blocked.lock().unwrap().is_empty());
    env.worker.shutdown().await;
}
