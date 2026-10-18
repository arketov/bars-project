use super::*;
use bars_protocol::{OperationEventsParams, OperationRetryParams};

fn since(epoch: &str, after: u64) -> OperationEventsParams {
    OperationEventsParams {
        epoch: Some(epoch.into()),
        after: Some(after),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn operation_short_job_delivers_its_own_applied_report_and_next_attempt_id() {
    let env = Env::new();
    let project = env.project("уведомления", &[("a.md", DOC)]);
    let worker = env.start();
    let first = worker.operation_events(&Default::default());
    worker.register(&project).unwrap();
    idle(&worker).await;
    let events = worker.operation_events(&since(&first.epoch, first.cursor));
    let result = events
        .completed
        .iter()
        .find(|op| op.project.is_some())
        .unwrap();
    assert_eq!(result.state, "success");
    assert_eq!(result.report.as_ref().unwrap().added, 1);
    assert!(result.report.as_ref().unwrap().index_applied);
    assert_eq!(documents(&project), 1, "итог после применения индекса");
    worker.reindex(&project, false, false).unwrap();
    idle(&worker).await;
    let next = worker.operation_events(&since(&events.epoch, events.cursor));
    let next = next
        .completed
        .iter()
        .find(|op| op.project.is_some())
        .unwrap();
    assert_ne!(result.id, next.id);
    assert_eq!(result.report.as_ref().unwrap().added, 1);
    assert_eq!(next.report.as_ref().unwrap().added, 0);
    assert!(
        worker
            .operation_events(&Default::default())
            .completed
            .is_empty(),
        "новое окно не получает старую историю"
    );
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn operation_queue_merge_cancel_retry_and_registration_identity() {
    let env = Env::new();
    let project = env.project("одинаковое имя", &[("a.md", DOC)]);
    let worker = env.start();
    worker.pause();
    let first = worker.operation_events(&Default::default());
    worker.register(&project).unwrap();
    worker.reindex(&project, false, false).unwrap();
    let queued = worker.operation_events(&since(&first.epoch, first.cursor));
    assert_eq!(queued.active.len(), 1);
    assert_eq!(queued.active[0].state, "queue_paused");
    let old_id = queued.active[0].id;
    worker.cancel(&project).unwrap();
    let cancelled = worker.operation_events(&since(&first.epoch, 0));
    assert_eq!(cancelled.completed[0].state, "cancelled");
    let retry = OperationRetryParams {
        epoch: first.epoch.clone(),
        id: old_id,
    };
    fs::write(
        env.home.path().join("settings.toml"),
        "[embed]\naccelerator='cpu'\n",
    )
    .unwrap();
    assert!(worker.retry_operation(&retry).is_err());
    assert!(
        !worker
            .operation_events(&since(&first.epoch, 0))
            .completed
            .iter()
            .find(|op| op.id == old_id)
            .unwrap()
            .target_available
    );
    fs::write(env.home.path().join("settings.toml"), "").unwrap();
    worker.retry_operation(&retry).unwrap();
    let next = worker.operation_events(&since(&first.epoch, cancelled.cursor));
    assert_ne!(next.active[0].id, old_id);
    worker.unregister(project.to_str().unwrap()).unwrap();
    worker.register(&project).unwrap();
    assert!(worker.retry_operation(&retry).is_err());
    let history = worker.operation_events(&since(&first.epoch, 0));
    assert!(
        !history
            .completed
            .iter()
            .find(|op| op.id == old_id)
            .unwrap()
            .target_available
    );
    worker.resume();
    idle(&worker).await;
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn operation_model_failure_is_explicit_safe_and_retry_is_new_attempt() {
    let env = Env::new();
    let worker = Worker::start(
        env.config(),
        Arc::new(FakeProvider {
            fail_load: true,
            ..FakeProvider::plain("m1")
        }),
    )
    .unwrap();
    let first = worker.operation_events(&Default::default());
    worker.start_download("BAAI/bge-m3", false);
    wait_until("явный итог модели", || {
        !worker
            .operation_events(&since(&first.epoch, 0))
            .completed
            .is_empty()
    })
    .await;
    let events = worker.operation_events(&since(&first.epoch, 0));
    let result = &events.completed[0];
    assert_eq!(result.state, "error");
    assert_eq!(result.kind, "download");
    assert!(!result.error.as_ref().unwrap().contains("backend test"));
    worker
        .retry_operation(&OperationRetryParams {
            epoch: first.epoch.clone(),
            id: result.id,
        })
        .unwrap();
    wait_until("повтор модели", || {
        worker
            .operation_events(&since(&first.epoch, events.cursor))
            .completed
            .iter()
            .any(|op| op.id != result.id)
    })
    .await;
    worker.shutdown().await;
}

#[test]
fn operation_journal_retention_gap_epoch_and_terminal_immutability() {
    let mut journal = super::super::operations::Journal::default();
    let first = journal.snapshot(&Default::default());
    let mut old = 0;
    for i in 0..1002 {
        let id = journal.begin(
            "check",
            "explicit",
            Some("проект".into()),
            1,
            None,
            None,
            false,
            0,
        );
        if i == 0 {
            old = id;
        }
        journal.finish(id, "success", None, None);
        journal.finish(id, "error", None, Some("поздняя ошибка".into()));
    }
    let output = journal.snapshot(&since(&first.epoch, 0));
    assert!(output.gap);
    assert_eq!(output.completed.len(), 1000);
    assert_eq!(output.cursor, 1002);
    assert!(!output.completed.iter().any(|op| op.id == old));
    assert!(output.completed.iter().all(|op| op.state == "success"));
    assert!(journal.snapshot(&since("другая эпоха", 0)).gap);
    assert!(journal.snapshot(&since(&first.epoch, 1003)).gap);
    assert!(!journal.snapshot(&since(&first.epoch, 1002)).gap);
}

#[tokio::test(flavor = "multi_thread")]
async fn operation_preparation_retry_after_terminal_failure_starts_a_new_attempt() {
    let env = Env::new();
    let worker = Worker::start(
        env.config(),
        Arc::new(FakeProvider {
            background: true,
            fail_load: true,
            ..FakeProvider::plain("m1")
        }),
    )
    .unwrap();
    let first = worker.operation_events(&Default::default());
    let settings = Settings::load(env.home.path(), env.projects.path())
        .unwrap()
        .value;
    let _ = worker.models().embed_readiness(&settings);
    wait_until("ошибка подготовки", || {
        !worker
            .operation_events(&since(&first.epoch, 0))
            .completed
            .is_empty()
    })
    .await;
    let result = worker.operation_events(&since(&first.epoch, 0));
    assert_eq!(result.completed[0].kind, "prepare");
    worker
        .retry_operation(&OperationRetryParams {
            epoch: first.epoch.clone(),
            id: result.completed[0].id,
        })
        .unwrap();
    wait_until("новая попытка подготовки", || {
        !worker
            .operation_events(&since(&first.epoch, result.cursor))
            .completed
            .is_empty()
    })
    .await;
    let next = worker.operation_events(&since(&first.epoch, result.cursor));
    assert_ne!(next.completed[0].id, result.completed[0].id);
    assert_eq!(next.completed[0].state, "error");
    worker.shutdown().await;
}

#[test]
fn operation_result_distinguishes_partial_cancel_and_unapplied_rebuild() {
    let mut journal = super::super::operations::Journal::default();
    let first = journal.snapshot(&Default::default());
    for (cancelled, rebuild) in [(false, false), (true, false), (false, true), (true, true)] {
        let id = journal.begin("check", "explicit", None, 0, None, None, false, 0);
        let report = UpdateReport {
            added: 2,
            rebuild_not_applied: rebuild,
            ..Default::default()
        };
        journal.index_result(
            id,
            &Outcome::Interrupted {
                report,
                cancelled,
                message: "secret=provider-token".into(),
            },
            rebuild,
        );
    }
    let output = journal.snapshot(&since(&first.epoch, 0));
    assert_eq!(
        output
            .completed
            .iter()
            .map(|op| op.state.as_str())
            .collect::<Vec<_>>(),
        ["partial", "cancelled", "partial", "cancelled"]
    );
    assert_eq!(
        output
            .completed
            .iter()
            .map(|op| op.report.as_ref().unwrap().index_applied)
            .collect::<Vec<_>>(),
        [true, true, false, false]
    );
    assert!(
        !serde_json::to_string(&output)
            .unwrap()
            .contains("provider-token")
    );
}
