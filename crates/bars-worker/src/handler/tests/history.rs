use super::*;
use crate::{handler::WorkerHandler, worker::Worker};
use interprocess::local_socket::traits::tokio::Listener as _;

#[tokio::test]
async fn mcp_history_ui_real_ipc_record_has_readable_input_and_result() {
    let harness = Harness::start("mcp-history-ui", FakeProvider::new("m1")).await;
    let mut cli = harness.client(ClientKind::Cli).await;
    let root = ready_project(&harness, &mut cli).await;
    let mut mcp = harness.client(ClientKind::Mcp).await;
    let original = serde_json::json!({"path":"guide.md","format":"json"});
    let response = mcp
        .call_observed(
            &ReadParams {
                lines: None,
                under: None,
                target: target(&root),
                path: "guide.md".into(),
            },
            Some(&original),
        )
        .await
        .unwrap();
    let history = crate::mcp_history::History::new(harness.home.path().into());
    let page = history.page(&Default::default()).unwrap();
    assert_eq!(page.records.len(), 1);
    let a = &page.records[0];
    assert_eq!(a.tool, "bars_read");
    assert_eq!(a.requested, original);
    assert_eq!(a.outcome, "success");
    assert_eq!(a.result["unit"], "lines");
    assert_eq!(a.result["count"], response.text.lines().count());
    assert_eq!(a.applied["range"], "whole_document");
    // Публикуем только данные временного проекта; локальное имя пользователя скрыто.
    let raw = serde_json::to_string(&page).unwrap();
    let escaped_root = serde_json::to_string(&root.display().to_string()).unwrap();
    let safe = raw.replace(
        &escaped_root[1..escaped_root.len() - 1],
        "C:/temporary-bars-project",
    );
    println!("MCP_UI_EVIDENCE={safe}");

    let partial_input = serde_json::json!({"path":"guide.md","lines":"2-4","format":"json"});
    mcp.call_observed(
        &ReadParams {
            target: target(&root),
            path: "guide.md".into(),
            lines: Some("2-4".into()),
            under: None,
        },
        Some(&partial_input),
    )
    .await
    .unwrap();
    let page = history.page(&Default::default()).unwrap();
    let partial = page
        .records
        .iter()
        .find(|a| a.requested == partial_input)
        .unwrap();
    assert_eq!(
        partial.applied["range"],
        serde_json::json!({"from":2,"to":4})
    );
    assert_eq!(partial.result["count"], 3);
}

async fn connection(
    endpoint: &Endpoint,
    handler: WorkerHandler,
) -> (Client, tokio::task::JoinHandle<std::io::Result<()>>) {
    let listener = crate::ipc::bind(endpoint).unwrap();
    let task = tokio::spawn(async move {
        let stream = listener.accept().await?;
        bars_protocol::serve_connection(stream, Arc::new(handler)).await
    });
    let client = Client::connect(endpoint, ClientKind::Mcp).await.unwrap();
    (client, task)
}

#[tokio::test]
async fn mcp_history_worker_restart_leaves_interrupted_ipc_unknown_and_repeat_is_new() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    fs::create_dir(project.path().join("docs")).unwrap();
    fs::write(project.path().join("docs/a.md"), "# Документ\n\nПоиск.\n").unwrap();
    let worker = Worker::start(
        WorkerConfig::new(home.path().into()).without_graph_pause(),
        FakeProvider::new("m1"),
    )
    .unwrap();
    worker.register(project.path()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status = worker.status(project.path()).unwrap();
        if status.updated_at.is_some() && status.state == ProjectState::Ready {
            break;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let handler = WorkerHandler::new(worker.clone());
    let history = handler.mcp_history();
    let endpoint = Endpoint::named("history-restart");
    let (mut client, server) = connection(&endpoint, handler.clone()).await;
    let gate = worker.gate(project.path()).unwrap().write_owned().await;
    let target = target(project.path());
    let params = SearchParams {
        diagnostic: false,
        target: target.clone(),
        query: "Поиск".into(),
        modules: vec![],
        files: vec![],
        limit: None,
        candidates: None,
        min_score: None,
        wait_models: None,
    };
    let request = params.clone();
    let call = tokio::spawn(async move { client.call(&request).await });
    let started = loop {
        let page = history.page(&Default::default()).unwrap();
        if let Some(a) = page.records.into_iter().find(|a| a.outcome == "running") {
            break a;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    assert!(!call.is_finished());
    // Обрываем только принадлежащий тесту сервер/канал во время исполнения.
    server.abort();
    let _ = server.await;
    let _ = call.await;
    drop(gate);
    worker.shutdown().await;
    drop(handler);
    let restarted = Worker::start(
        WorkerConfig::new(home.path().into()).without_graph_pause(),
        FakeProvider::new("m1"),
    )
    .unwrap();
    let handler = WorkerHandler::new(restarted.clone());
    let recovered = handler.mcp_history();
    let rows = recovered.page(&Default::default()).unwrap().records;
    let interrupted = rows.iter().find(|a| a.id == started.id).unwrap();
    assert_eq!(interrupted.outcome, "unknown");
    assert!(interrupted.finished.is_none());
    assert!(interrupted.duration_ms.is_none());
    let (mut client, server) = connection(&endpoint, handler).await;
    // Повторяем тот же допущенный запрос; успех или model_loading — новый известный исход.
    let _ = client.call(&params).await;
    let rows = recovered.page(&Default::default()).unwrap().records;
    let repeated = &rows[0];
    assert_ne!(repeated.id, started.id);
    assert_ne!(repeated.epoch, started.epoch);
    assert_ne!(repeated.session, started.session);
    assert!(matches!(repeated.outcome.as_str(), "success" | "error"));
    drop(client);
    server.abort();
    let _ = server.await;
    restarted.shutdown().await;
}
