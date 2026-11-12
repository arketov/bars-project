//! ISS-16: полный поиск и управление на настоящем IPC; профиль/индекс только временные.
use std::sync::Arc;
use std::time::{Duration, Instant};

use bars_protocol::{
    Call, Client, ClientKind, Endpoint, ReindexParams, SearchParams, StatusParams, Target,
};
use bars_worker::handler::WorkerHandler;
use bars_worker::worker::{ConfiguredProvider, Worker, WorkerConfig};

#[tokio::main]
async fn main() {
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    std::fs::create_dir(root.join("docs")).unwrap();
    for i in 0..24 {
        std::fs::write(root.join(format!("docs/{i:02}.md")), format!("---\ntitle: Документ {i}\n---\n# Документ {i}\n\nИндексация документов, поиск и проверка состояния проекта. Раздел номер {i}.\n")).unwrap();
    }
    std::fs::write(
        home.path().join("settings.toml"),
        "[embed]\nmodel = \"BAAI/bge-m3\"\n[rerank]\nenabled = true\n",
    )
    .unwrap();
    let models =
        std::path::PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(".bars/models");
    let settings = bars_core::settings::Settings::load(home.path(), root)
        .unwrap()
        .value;
    let provider = Arc::new(ConfiguredProvider::new(models));
    use bars_worker::worker::ModelProvider;
    for id in [&settings.embed.model, &settings.rerank.model] {
        assert_eq!(
            provider.download_percent(id, &settings),
            Some(100),
            "Нет готовых весов {id}; стенд ничего не скачивает"
        );
    }
    let mut config = WorkerConfig::new(home.path().into());
    config.periodic_interval = Duration::from_secs(3600);
    let worker = Worker::start(config, provider).unwrap();
    let handler = WorkerHandler::new(worker.clone());
    let endpoint = Endpoint::named(&format!("iss16-probe-{}", std::process::id()));
    let listener = bars_worker::ipc::bind(&endpoint).unwrap();
    let server = tokio::spawn(bars_worker::ipc::run_server(
        listener,
        Arc::new(handler.clone()),
    ));
    worker.register(root).unwrap();
    let deadline = Instant::now() + Duration::from_secs(300);
    while !worker.is_idle() {
        assert!(Instant::now() < deadline, "initial indexing timed out");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        worker.status(root).unwrap().last_error.is_none(),
        "{:?}",
        worker.status(root)
    );
    let target = Target {
        cwd: root.display().to_string(),
        project: None,
        wt: None,
    };
    let query = SearchParams {
        diagnostic: true,
        target: target.clone(),
        query: "Как проверить индексацию документов?".into(),
        modules: vec![],
        files: vec![],
        paths: vec![],
        limit: Some(3),
        candidates: None,
        min_score: Some(0.0),
        wait_models: None,
    };
    let mut client = Client::connect(&endpoint, ClientKind::Mcp).await.unwrap();
    // Прогрев rerank, если он ещё загружается в фоне.
    loop {
        let out = client.call(&query).await.unwrap();
        if out.rerank_error.is_none() && format!("{:?}", out.applied.score_kind).contains("Rerank")
        {
            break;
        }
        assert!(Instant::now() < deadline, "rerank did not become ready");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    println!("runtime={:?}", worker.model_runtime());
    let started = Instant::now();
    let idle = client.call(&query).await.unwrap();
    println!(
        "idle_search_ms={} hits={} score={:?} error={:?}",
        started.elapsed().as_millis(),
        idle.results.len(),
        idle.applied.score_kind,
        idle.rerank_error
    );
    worker.reindex(root, true, false).unwrap();
    while !worker.progress().is_some_and(|p| p.current_file.is_some()) {
        assert!(Instant::now() < deadline, "rebuild did not start");
        tokio::task::yield_now().await;
    }
    std::fs::write(
        root.join("docs/new.md"),
        "# Новый документ\n\nИзменение после начала полной индексации.\n",
    )
    .unwrap();
    let started = Instant::now();
    let status = client
        .call(&StatusParams {
            target: target.clone(),
        })
        .await
        .unwrap();
    println!(
        "status_ms={} state={:?}",
        started.elapsed().as_millis(),
        status.state
    );
    for call in [
        Call::Activity(bars_protocol::ActivityParams {}),
        Call::OperationEvents(Default::default()),
    ] {
        let method = call.method();
        let started = Instant::now();
        handler.dispatch(call).await.unwrap();
        println!("{method}_ms={}", started.elapsed().as_millis());
    }
    let started = Instant::now();
    handler
        .dispatch(Call::Reindex(ReindexParams {
            target: target.clone(),
            full: false,
            accept_model_change: false,
        }))
        .await
        .unwrap();
    println!(
        "check_accepted_ms={} queued={}",
        started.elapsed().as_millis(),
        worker.status(root).unwrap().queued
    );
    let epoch = worker.operation_events(&Default::default()).epoch;
    loop {
        let events = worker.operation_events(&bars_protocol::OperationEventsParams {
            epoch: Some(epoch.clone()),
            after: Some(0),
        });
        if let Some(scan) = events.completed.iter().find(|op| op.kind == "scan") {
            println!(
                "check_completed_ms={} state={} report={:?} running_after={:?}",
                started.elapsed().as_millis(),
                scan.state,
                scan.report,
                worker.progress().map(|p| p.kind)
            );
            assert_eq!(scan.state, "success");
            assert_eq!(scan.report.as_ref().unwrap().added, 1);
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "scan did not finish independently"
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    for mode in ["ipc", "window"] {
        let started = Instant::now();
        let out: bars_protocol::SearchOutput = if mode == "ipc" {
            client.call(&query).await.unwrap()
        } else {
            serde_json::from_value(handler.dispatch(Call::Search(query.clone())).await.unwrap())
                .unwrap()
        };
        println!(
            "{mode}_search_ms={} hits={} indexing={} score={:?} error={:?} running_after={:?}",
            started.elapsed().as_millis(),
            out.results.len(),
            out.indexing,
            out.applied.score_kind,
            out.rerank_error,
            worker.progress().map(|p| p.kind)
        );
    }
    worker.shutdown().await;
    server.abort();
}
