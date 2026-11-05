//! Методы окна: настройки, модели, ключи, клиенты, корень документации. Окно вызывает обработчик
//! напрямую, поэтому тесты берут `dispatch` без канала; хранилище ключей — в памяти, клиенты и
//! `~/.bars` — во временных папках.

use bars_core::secrets::MemorySecrets;
use bars_protocol::{
    AutostartGetParams, AutostartOutput, AutostartSetParams, CheckAllParams, ClientsStatusOutput,
    ClientsStatusParams, InstallClient, InstallParams, InstructionOutput, InstructionParams,
    ModelDownloadParams, ModelsInfoOutput, ModelsInfoParams, SecretOutput, SecretSetParams,
    SecretStatusParams, SetDocsRootParams, SettingsGetParams, SettingsOutput, SettingsSaveParams,
};
use serde::de::DeserializeOwned;

use super::*;
use crate::autostart::MemoryAutostart;
use crate::handler::WorkerHandler;
use crate::worker::Worker;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn operation_events_window_dispatch_keeps_short_terminal_outcome_and_retry() {
    use bars_protocol::{
        AcceptedOutput, OperationEventsOutput, OperationEventsParams, OperationRetryParams,
    };
    let env = Window::new();
    let call = Call::OperationEvents(OperationEventsParams::default());
    assert!(call.window_only());
    assert!(!call.allowed_for(ClientKind::Mcp));
    assert!(!call.allowed_for(ClientKind::Cli));
    let initial: OperationEventsOutput = env.call(call).await;
    let project = env.project("короткая операция", "docs");
    env.worker.pause();
    env.worker.register(&project).unwrap();
    let params = OperationEventsParams {
        epoch: Some(initial.epoch.clone()),
        after: Some(initial.cursor),
    };
    let queued: OperationEventsOutput = env.call(Call::OperationEvents(params.clone())).await;
    assert_eq!(queued.active.len(), 1);
    env.worker.cancel(&project).unwrap();
    let cancelled: OperationEventsOutput = env.call(Call::OperationEvents(params.clone())).await;
    assert_eq!(cancelled.completed[0].state, "cancelled");
    let retried: AcceptedOutput = env
        .call(Call::OperationRetry(OperationRetryParams {
            epoch: initial.epoch,
            id: cancelled.completed[0].id,
        }))
        .await;
    assert!(retried.accepted);
    env.worker.resume();
    env.idle().await;
    let result: OperationEventsOutput = env.call(Call::OperationEvents(params)).await;
    assert!(result.completed.iter().any(|op| op.project.is_some()
        && op.state == "success"
        && op.report.as_ref().unwrap().index_applied));
    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn physical_graph_slice_is_window_only_and_expands_chunks_on_the_same_snapshot() {
    use bars_core::graph::LayerState;
    use bars_core::graph::physical::{ExpandedDocument, PhysicalEdgeKind, PhysicalNodeKind};
    use bars_protocol::{
        GraphPhysicalParams, GraphStatus, GraphStatusParams, PhysicalGraphSlice, PhysicalRequest,
        Target,
    };
    let env = Window::new();
    let root = env.projects.path().join("физика");
    fs::create_dir_all(root.join("docs/features")).unwrap();
    fs::write(
        root.join("docs/features/a.md"),
        "# Альфа\n\nТекст про индексацию.\n\n## Раздел\n\nСсылка на [бета](b.md) и ещё раз [бета](b.md#бета).\n",
    )
    .unwrap();
    fs::write(
        root.join("docs/features/b.md"),
        "# Бета\n\nОбратная ссылка на [альфа](a.md).\n",
    )
    .unwrap();
    fs::write(root.join("docs/lonely.md"), "# Один\n\nБез ссылок.\n").unwrap();
    let target = Target {
        cwd: root.display().to_string(),
        project: Some(root.display().to_string()),
    };
    let params = |request: PhysicalRequest| {
        Call::GraphPhysical(GraphPhysicalParams {
            target: target.clone(),
            request,
        })
    };
    let call = params(PhysicalRequest::default());
    assert!(call.window_only());
    assert!(!call.allowed_for(ClientKind::Mcp));
    assert!(!call.allowed_for(ClientKind::Cli));
    env.worker.register(&root).unwrap();
    env.idle().await;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let status: GraphStatus = env
            .call(Call::GraphStatus(GraphStatusParams {
                target: target.clone(),
            }))
            .await;
        if status.state == LayerState::Ready {
            break;
        }
        assert!(Instant::now() < deadline, "граф не построился: {status:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let slice: PhysicalGraphSlice = env.call(call).await;
    assert_eq!(slice.method, "physical-v1");
    let documents: Vec<_> = slice
        .nodes
        .iter()
        .filter(|n| n.kind == PhysicalNodeKind::Document)
        .collect();
    assert_eq!(documents.len(), 3);
    assert!(
        slice
            .nodes
            .iter()
            .all(|n| n.kind == PhysicalNodeKind::Document)
    );
    let links: Vec<_> = slice
        .edges
        .iter()
        .filter(|e| e.kind == PhysicalEdgeKind::MarkdownLink)
        .collect();
    assert_eq!(links.len(), 2, "A→B и B→A, повтор ссылки не создаёт ребра");
    assert!(links.iter().any(|e| e.lines == vec![7]), "{links:?}");
    assert!(!slice.truncated);
    let alpha = documents
        .iter()
        .find(|n| n.docs_path == "features/a.md")
        .unwrap();
    // Вид запроса имеет отдельную команду, не зависит от лимита общего среза и доступен только окну.
    let query_request = bars_protocol::QuerySliceRequest {
        id: "search-1".into(),
        text: "индексация".into(),
        scope: bars_core::graph::Scope {
            modules: vec!["features".into()],
            files: vec![],
        },
        hits: vec![bars_core::graph::query_slice::QueryHit {
            source_hash: String::new(),
            channel: Default::default(),
            path: alpha.path.clone(),
            title: alpha.label.clone(),
            heading: "Альфа".into(),
            first: 1,
            last: 7,
            score: Some(0.83),
        }],
        score_kind: bars_core::graph::query_slice::QueryScoreKind::Cosine,
        depth: 1,
        fanout: 4,
        min_strength: 0.5,
        max_nodes: 200,
        max_edges: 800,
        max_bytes: 262144,
        snapshot: Some(slice.snapshot.clone()),
    };
    let query_call = Call::GraphQuerySlice(bars_protocol::GraphQuerySliceParams {
        target: target.clone(),
        request: query_request.clone(),
    });
    assert!(query_call.window_only());
    assert!(!query_call.allowed_for(ClientKind::Mcp));
    assert!(!query_call.allowed_for(ClientKind::Cli));
    let projected: bars_protocol::QueryGraphSlice = env.call(query_call).await;
    assert_eq!(projected.physical.snapshot, slice.snapshot);
    assert_eq!(projected.results[0].score, Some(0.83));
    assert!(
        projected
            .physical
            .nodes
            .iter()
            .all(|n| n.module == "features")
    );
    assert_eq!(
        projected.physical.nodes.len(),
        2,
        "ссылка включает второй документ области"
    );
    let mut expired = query_request;
    expired.snapshot = Some("expired".into());
    let rejected = env
        .error(Call::GraphQuerySlice(
            bars_protocol::GraphQuerySliceParams {
                target: target.clone(),
                request: expired,
            },
        ))
        .await;
    assert!(rejected.message.contains("снимок"));
    assert!(alpha.chunks >= 1);
    // Раскрытие того же документа на том же снимке добавляет его чанки с номерами строк источника.
    let expanded: PhysicalGraphSlice = env
        .call(params(PhysicalRequest {
            snapshot: Some(slice.snapshot.clone()),
            expanded: vec![ExpandedDocument {
                document: alpha.id.clone(),
                shown: 50,
            }],
            ..Default::default()
        }))
        .await;
    assert_eq!(expanded.snapshot, slice.snapshot);
    let chunks: Vec<_> = expanded
        .nodes
        .iter()
        .filter(|n| n.kind == PhysicalNodeKind::Chunk)
        .collect();
    assert_eq!(chunks.len(), alpha.chunks);
    assert!(
        chunks
            .iter()
            .all(|c| c.owner == alpha.id && c.first_line >= 1)
    );
    // Устаревший снимок не склеивается со свежим.
    let error = env
        .error(params(PhysicalRequest {
            snapshot: Some("старый".into()),
            ..Default::default()
        }))
        .await;
    assert!(error.message.contains("снимок"), "{error:?}");
    let index = bars_core::storage::Index::open(
        &bars_core::storage::index_path(&root),
        bars_core::storage::Journal::Wal,
    )
    .unwrap();
    index
        .conn()
        .execute(
            "UPDATE documents SET hash='changed' WHERE path='docs/features/a.md'",
            [],
        )
        .unwrap();
    let previous: PhysicalGraphSlice = env.call(params(PhysicalRequest::default())).await;
    assert_eq!(previous.snapshot, slice.snapshot);
    assert_eq!(previous.layers.document_similarity.state, LayerState::Stale);
    assert_eq!(previous.layers.markdown_link.state, LayerState::Stale);
    assert!(previous.warnings.iter().any(|w| w.contains("Граф устарел")));
    drop(index);
    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn project_deletion_requires_confirmation_and_is_window_only() {
    use bars_protocol::ProjectDeleteParams;
    let env = Window::new();
    let project = env.project("delete", "docs");
    env.worker.register(&project).unwrap();
    env.idle().await;
    let params = ProjectDeleteParams {
        project: project.display().to_string(),
        confirmed: false,
    };
    assert!(Call::ProjectDelete(params.clone()).window_only());
    assert!(!Call::ProjectDelete(params.clone()).allowed_for(ClientKind::Mcp));
    assert!(!Call::ProjectDelete(params.clone()).allowed_for(ClientKind::Cli));
    let error = env.error(Call::ProjectDelete(params.clone())).await;
    assert_eq!(error.kind(), Some(ErrorKind::InvalidParams));
    assert!(project.join(".bars/index.sqlite").is_file());
    let output: bars_protocol::UnregisterOutput = env
        .call(Call::ProjectDelete(ProjectDeleteParams {
            confirmed: true,
            ..params
        }))
        .await;
    assert!(output.removed);
    assert!(!project.join(".bars").exists());
    assert!(project.join("docs/a.md").is_file());
    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn project_rename_updates_tools_without_changing_path_or_index() {
    use bars_protocol::{ProjectRenameParams, ProjectsOutput, ProjectsParams, SavedOutput};
    let env = Window::new();
    let project = env.project("repo", "docs");
    env.worker.register(&project).unwrap();
    env.idle().await;
    let index = project.join(".bars/index.sqlite");
    let before = fs::read(&index).unwrap();
    let params = ProjectRenameParams {
        project: project.display().to_string(),
        name: "  Мои знания  ".into(),
    };
    assert!(Call::ProjectRename(params.clone()).window_only());
    let saved: SavedOutput = env.call(Call::ProjectRename(params.clone())).await;
    assert!(saved.saved);
    let output: ProjectsOutput = env.call(Call::Projects(ProjectsParams::default())).await;
    assert_eq!(output.projects[0].name, "Мои знания");
    assert_eq!(output.projects[0].path, params.project);
    assert_eq!(env.worker.resolve("Мои знания").unwrap(), project);
    assert!(env.worker.resolve("repo").is_err());
    assert_eq!(fs::read(&index).unwrap(), before);
    for name in ["  ", "имя\nпроекта"] {
        let error = env
            .error(Call::ProjectRename(ProjectRenameParams {
                name: name.into(),
                ..params.clone()
            }))
            .await;
        assert_eq!(error.kind(), Some(ErrorKind::InvalidParams));
    }
    assert_eq!(env.worker.projects()[0].name, "Мои знания");
    env.worker.shutdown().await;
}

struct Window {
    home: TempDir,
    projects: TempDir,
    provider: Arc<FakeProvider>,
    worker: Worker,
    handler: WorkerHandler,
}

impl Window {
    fn new() -> Self {
        let home = TempDir::new().unwrap();
        let provider = FakeProvider::new("m1");
        let mut config = WorkerConfig::new(home.path().to_path_buf()).without_graph_pause();
        config.install_env = Some(InstallEnv {
            home: home.path().join("client-home"),
            iterion_home: None,
            mcp_exe: PathBuf::from("/opt/bars/bars-mcp"),
            cli_exe: PathBuf::from("/opt/bars/bars-cli"),
        });
        config.secrets = Arc::new(MemorySecrets::default());
        // Реестр Windows тесты не трогают: запись автозапуска в памяти.
        config.autostart = Arc::new(MemoryAutostart::new(
            "\"/opt/bars/bars\" --background",
            None,
        ));
        let worker = Worker::start(config, provider.clone()).unwrap();
        let handler = WorkerHandler::new(worker.clone());
        Self {
            home,
            projects: TempDir::new().unwrap(),
            provider,
            worker,
            handler,
        }
    }

    async fn call<T: DeserializeOwned>(&self, call: Call) -> T {
        serde_json::from_value(self.handler.dispatch(call).await.unwrap()).unwrap()
    }

    async fn error(&self, call: Call) -> RpcError {
        self.handler.dispatch(call).await.unwrap_err()
    }

    fn project(&self, name: &str, folder: &str) -> PathBuf {
        let root = self.projects.path().join(name);
        fs::create_dir_all(root.join(folder)).unwrap();
        fs::write(
            root.join(folder).join("a.md"),
            "# Альфа\n\nТекст про индексацию.\n",
        )
        .unwrap();
        root
    }

    async fn idle(&self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        // Очередь могла ещё не получить задание: короткая пауза и проверка несколько раз подряд.
        loop {
            tokio::time::sleep(Duration::from_millis(60)).await;
            if self.worker.is_idle() {
                tokio::time::sleep(Duration::from_millis(60)).await;
                if self.worker.is_idle() {
                    return;
                }
            }
            assert!(Instant::now() < deadline, "очередь не опустела");
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn settings_roundtrip_through_the_handler() {
    let window = Window::new();
    let output: SettingsOutput = window.call(Call::SettingsGet(SettingsGetParams {})).await;
    assert_eq!(output.settings.embed.source, "builtin");
    assert!(output.file.ends_with("settings.toml"));

    let mut settings = output.settings.clone();
    settings.search.results = 9;
    settings.rerank.enabled = true;
    let saved: bars_protocol::SettingsSaveOutput = window
        .call(Call::SettingsSave(SettingsSaveParams::between(
            output.clone(),
            settings.clone(),
        )))
        .await;
    assert_eq!(saved.status, bars_protocol::SettingsSaveStatus::Saved);
    let again: SettingsOutput = window.call(Call::SettingsGet(SettingsGetParams {})).await;
    assert_eq!(again.settings.search.results, 9);
    assert!(again.settings.rerank.enabled);

    settings.chunking.min_chars = 0;
    let error = window
        .error(Call::SettingsSave(SettingsSaveParams::between(
            again, settings,
        )))
        .await;
    assert_eq!(error.kind(), Some(ErrorKind::InvalidParams));
    window.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn saving_settings_puts_projects_with_another_model_into_reindex_required() {
    let window = Window::new();
    let root = window.project("alpha", "docs");
    window.worker.register(&root).unwrap();
    window.idle().await;
    assert_eq!(
        window.worker.status(&root).unwrap().state,
        ProjectState::Ready
    );

    // «Другая модель в настройках»: подделка меняет отпечаток, сохранение запускает проверку.
    window.provider.set_model("m2");
    let settings = window
        .call::<SettingsOutput>(Call::SettingsGet(SettingsGetParams {}))
        .await;
    window
        .call::<bars_protocol::SettingsSaveOutput>(Call::SettingsSave(SettingsSaveParams::between(
            settings.clone(),
            settings.settings.clone(),
        )))
        .await;
    window.idle().await;
    let status = window.worker.status(&root).unwrap();
    assert_eq!(status.state, ProjectState::ReindexRequired);
    window.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn reranker_download_marker_follows_selected_precision() {
    let window = Window::new();
    let snapshot = window
        .home
        .path()
        .join("models/models--rozgo--bge-reranker-v2-m3/snapshots/cpu");
    fs::create_dir_all(&snapshot).unwrap();
    for file in [
        "config.json",
        "tokenizer.json",
        "tokenizer_config.json",
        "special_tokens_map.json",
        "model.onnx",
        "model.onnx.data",
    ] {
        fs::write(snapshot.join(file), "w").unwrap();
    }
    let auto: ModelsInfoOutput = window.call(Call::ModelsInfo(ModelsInfoParams {})).await;
    if cfg!(windows) {
        assert!(
            !auto.rerank[0].downloaded,
            "FP32-кэш не является готовым FP16"
        );
    }
    fs::write(
        window.home.path().join("settings.toml"),
        "[rerank]\naccelerator='cpu'\n",
    )
    .unwrap();
    let cpu: ModelsInfoOutput = window.call(Call::ModelsInfo(ModelsInfoParams {})).await;
    assert!(cpu.rerank[0].downloaded, "CPU не требует GPU-весов");
    window.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn models_info_reports_downloaded_files_and_download_starts() {
    let window = Window::new();
    let snapshot = window
        .home
        .path()
        .join("models/models--BAAI--bge-m3/snapshots/rev");
    fs::create_dir_all(snapshot.join("onnx")).unwrap();
    for file in [
        "config.json",
        "special_tokens_map.json",
        "tokenizer.json",
        "tokenizer_config.json",
        "onnx/model.onnx",
        "onnx/model.onnx_data",
        "onnx/Constant_7_attr__value",
    ] {
        fs::write(snapshot.join(file), "w").unwrap();
    }

    let info: ModelsInfoOutput = window.call(Call::ModelsInfo(ModelsInfoParams {})).await;
    assert_eq!(info.embed.len(), bars_core::models::builtin_models().len());
    let bge = &info.embed[0];
    assert_eq!(bge.id, "BAAI/bge-m3");
    assert!(bge.downloaded && !bge.downloading);
    assert_eq!(bge.download_percent, Some(100));
    assert_eq!(bge.download_error, None);
    assert_eq!((bge.dim, bge.backend.as_str()), (1024, "onnx"));
    let qwen = info
        .embed
        .iter()
        .find(|m| m.id.contains("Qwen3-Embedding-0.6B"))
        .unwrap();
    assert!(!qwen.downloaded && qwen.backend == "gguf" && !qwen.warnings.is_empty());
    assert_eq!(qwen.download_percent, Some(0));
    assert_eq!(info.rerank[0].id, "BAAI/bge-reranker-v2-m3");

    let unknown = window
        .error(Call::ModelDownload(ModelDownloadParams {
            id: "нет/такой".into(),
        }))
        .await;
    assert_eq!(unknown.kind(), Some(ErrorKind::InvalidParams));
    window
        .call::<bars_protocol::AcceptedOutput>(Call::ModelDownload(ModelDownloadParams {
            id: "Qwen/Qwen3-Embedding-0.6B".into(),
        }))
        .await;
    // Подделка «скачивает» мгновенно: признак скачивания снимается сам.
    let deadline = Instant::now() + Duration::from_secs(10);
    while !window.worker.downloading().is_empty() {
        assert!(Instant::now() < deadline, "скачивание не закончилось");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    window
        .call::<bars_protocol::AcceptedOutput>(Call::ModelRetry(bars_protocol::ModelRetryParams {}))
        .await;
    window.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn secret_is_stored_and_never_returned() {
    let window = Window::new();
    let status = |name: &str| {
        Call::SecretStatus(SecretStatusParams {
            name: name.to_string(),
        })
    };
    let before: SecretOutput = window.call(status("bars-embed")).await;
    assert!(!before.present);

    let set = SecretSetParams {
        name: "bars-embed".into(),
        value: "sk-test-not-a-real-key".into(),
    };
    assert!(
        !format!("{set:?}").contains("sk-test"),
        "ключ не должен попадать в Debug"
    );
    let stored: SecretOutput = window.call(Call::SecretSet(set)).await;
    assert!(stored.present);
    let after: SecretOutput = window.call(status("bars-embed")).await;
    assert!(after.present);

    let empty = window
        .error(Call::SecretSet(SecretSetParams {
            name: " ".into(),
            value: "x".into(),
        }))
        .await;
    assert_eq!(empty.kind(), Some(ErrorKind::InvalidParams));
    window.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn clients_status_follows_install_and_instruction_names_the_exes() {
    let window = Window::new();
    let before: ClientsStatusOutput = window
        .call(Call::ClientsStatus(ClientsStatusParams {}))
        .await;
    assert_eq!(before.clients.len(), 4);
    assert!(before.clients.iter().all(|c| !c.installed && !c.stale));

    window
        .call::<bars_protocol::InstallOutput>(Call::Install(InstallParams {
            client: InstallClient::Codex,
        }))
        .await;
    let after: ClientsStatusOutput = window
        .call(Call::ClientsStatus(ClientsStatusParams {}))
        .await;
    let codex = after
        .clients
        .iter()
        .find(|c| c.client == InstallClient::Codex)
        .unwrap();
    assert!(codex.installed && codex.needs_hook_approval && !codex.files.is_empty());
    let claude = after
        .clients
        .iter()
        .find(|c| c.client == InstallClient::ClaudeCode)
        .unwrap();
    assert!(!claude.installed && !claude.needs_hook_approval);
    let kimi = after
        .clients
        .iter()
        .find(|c| c.client == InstallClient::KimiCode)
        .unwrap();
    assert!(!kimi.installed && !kimi.needs_hook_approval);

    let instruction: InstructionOutput = window.call(Call::Instruction(InstructionParams {})).await;
    assert!(instruction.text.contains("/opt/bars/bars-cli"));
    assert!(instruction.mcp_config.contains("/opt/bars/bars-mcp"));
    window.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn another_docs_root_is_written_and_indexed() {
    let window = Window::new();
    let root = window.project("alpha", "documentation");
    // Корень выбран `docs/`, документы лежат в `documentation/`; без выбора корнем стала бы
    // вся папка (Т-7).
    fs::create_dir_all(root.join(".bars")).unwrap();
    fs::write(root.join(".bars/config.toml"), "docs_root = \"docs\"\n").unwrap();
    window.worker.register(&root).unwrap();
    window.idle().await;
    assert!(
        window
            .call::<bars_protocol::TreeOutput>(Call::Tree(bars_protocol::TreeParams {
                target: target(&root)
            }))
            .await
            .documents
            .is_empty(),
        "корень `docs` пуст"
    );

    window
        .call::<bars_protocol::AcceptedOutput>(Call::SetDocsRoot(SetDocsRootParams {
            project: root.display().to_string(),
            docs_root: "documentation".into(),
        }))
        .await;
    window.idle().await;
    let tree: bars_protocol::TreeOutput = window
        .call(Call::Tree(bars_protocol::TreeParams {
            target: target(&root),
        }))
        .await;
    assert_eq!(tree.docs_root, "documentation");
    assert_eq!(tree.documents.len(), 1);
    assert!(
        fs::read_to_string(root.join(".bars/config.toml"))
            .unwrap()
            .contains("documentation")
    );

    let empty = window
        .error(Call::SetDocsRoot(SetDocsRootParams {
            project: root.display().to_string(),
            docs_root: "  ".into(),
        }))
        .await;
    assert_eq!(empty.kind(), Some(ErrorKind::InvalidParams));
    window
        .call::<bars_protocol::AcceptedOutput>(Call::CheckAll(CheckAllParams {}))
        .await;
    window.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn autostart_is_off_by_default_and_toggles() {
    let window = Window::new();
    let state: AutostartOutput = window.call(Call::AutostartGet(AutostartGetParams {})).await;
    assert!(state.supported && !state.enabled && !state.stale);

    let on: AutostartOutput = window
        .call(Call::AutostartSet(AutostartSetParams { enabled: true }))
        .await;
    assert!(on.enabled);
    assert_eq!(
        on.command.as_deref(),
        Some("\"/opt/bars/bars\" --background")
    );
    let off: AutostartOutput = window
        .call(Call::AutostartSet(AutostartSetParams { enabled: false }))
        .await;
    assert!(!off.enabled);
    window.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn autostart_set_is_refused_where_the_os_has_none() {
    let home = TempDir::new().unwrap();
    let config = WorkerConfig::new(home.path().to_path_buf()).without_graph_pause();
    let worker = Worker::start(config, FakeProvider::new("m1")).unwrap();
    let handler = WorkerHandler::new(worker.clone());
    let state: AutostartOutput = serde_json::from_value(
        handler
            .dispatch(Call::AutostartGet(AutostartGetParams {}))
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(!state.supported);
    let error = handler
        .dispatch(Call::AutostartSet(AutostartSetParams { enabled: true }))
        .await
        .unwrap_err();
    assert_eq!(error.kind(), Some(ErrorKind::InvalidParams));
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn query_slice_keeps_historical_scope_without_widening() {
    use bars_core::graph::{
        LayerState, Scope,
        query_slice::{QueryHit, QueryScoreKind, QuerySourceState},
    };
    use bars_protocol::{GraphQuerySliceParams, GraphStatus, GraphStatusParams, QuerySliceRequest};
    let env = Window::new();
    let root = env.project("query-history", "docs");
    env.worker.register(&root).unwrap();
    env.idle().await;
    let target = target(&root);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let status: GraphStatus = env
            .call(Call::GraphStatus(GraphStatusParams {
                target: target.clone(),
            }))
            .await;
        if status.state == LayerState::Ready {
            break;
        }
        assert!(Instant::now() < deadline, "граф не построился: {status:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let request = QuerySliceRequest {
        id: "old-search".into(),
        text: "удалённый документ".into(),
        scope: Scope {
            modules: vec!["removed".into()],
            files: vec!["removed/gone.md".into()],
        },
        hits: vec![QueryHit {
            source_hash: "saved-revision".into(),
            channel: Default::default(),
            path: "docs/removed/gone.md".into(),
            title: "Удалённый".into(),
            heading: String::new(),
            first: 1,
            last: 1,
            score: Some(0.8),
        }],
        score_kind: QueryScoreKind::Cosine,
        depth: 1,
        fanout: 4,
        min_strength: 0.5,
        max_nodes: 200,
        max_edges: 800,
        max_bytes: 262144,
        snapshot: None,
    };
    let reply: bars_protocol::QueryGraphSlice = env
        .call(Call::GraphQuerySlice(GraphQuerySliceParams {
            target,
            request,
        }))
        .await;
    assert_eq!(
        reply.results[0].status,
        QuerySourceState::DeletedSinceSearch
    );
    assert!(reply.results[0].node.is_some());
    assert_eq!(
        reply.physical.totals.documents, 0,
        "область не расширилась до всего проекта"
    );
    assert_eq!(
        reply.physical.nodes.len(),
        1,
        "только сохранённый результат-призрак"
    );
    assert!(reply.physical.edges.is_empty());
    env.worker.shutdown().await;
}
