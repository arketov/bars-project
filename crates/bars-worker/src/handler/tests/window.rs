//! Методы окна: настройки, модели, ключи, клиенты, корень документации. Окно вызывает обработчик
//! напрямую, поэтому тесты берут `dispatch` без канала; хранилище ключей — в памяти, клиенты и
//! `~/.bars` — во временных папках.

use bars_core::secrets::MemorySecrets;
use bars_protocol::{
    AutostartGetParams, AutostartOutput, AutostartSetParams, CheckAllParams, ClientsStatusOutput,
    ClientsStatusParams, InstallClient, InstallParams, InstructionOutput, InstructionParams,
    ModelDownloadParams, ModelsInfoOutput, ModelsInfoParams, SavedOutput, SecretOutput,
    SecretSetParams, SecretStatusParams, SetDocsRootParams, SettingsGetParams, SettingsOutput,
    SettingsSaveParams,
};
use serde::de::DeserializeOwned;

use super::*;
use crate::autostart::MemoryAutostart;
use crate::handler::WorkerHandler;
use crate::worker::Worker;

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
        let mut config = WorkerConfig::new(home.path().to_path_buf());
        config.install_env = Some(InstallEnv {
            home: home.path().join("client-home"),
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

    let mut settings = output.settings;
    settings.search.results = 9;
    settings.rerank.enabled = true;
    let saved: SavedOutput = window
        .call(Call::SettingsSave(SettingsSaveParams {
            settings: settings.clone(),
        }))
        .await;
    assert!(saved.saved);
    let again: SettingsOutput = window.call(Call::SettingsGet(SettingsGetParams {})).await;
    assert_eq!(again.settings.search.results, 9);
    assert!(again.settings.rerank.enabled);

    settings.chunking.min_chars = 0;
    let error = window
        .error(Call::SettingsSave(SettingsSaveParams { settings }))
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
        .await
        .settings;
    window
        .call::<SavedOutput>(Call::SettingsSave(SettingsSaveParams { settings }))
        .await;
    window.idle().await;
    let status = window.worker.status(&root).unwrap();
    assert_eq!(status.state, ProjectState::ReindexRequired);
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
    assert_eq!(info.embed.len(), 4);
    let bge = &info.embed[0];
    assert_eq!(bge.id, "BAAI/bge-m3");
    assert!(bge.downloaded && !bge.downloading);
    assert_eq!((bge.dim, bge.backend.as_str()), (1024, "onnx"));
    let qwen = info
        .embed
        .iter()
        .find(|m| m.id.contains("Qwen3-Embedding-0.6B"))
        .unwrap();
    assert!(!qwen.downloaded && qwen.backend == "candle" && !qwen.warnings.is_empty());
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
    assert_eq!(before.clients.len(), 2);
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

    let instruction: InstructionOutput = window.call(Call::Instruction(InstructionParams {})).await;
    assert!(instruction.text.contains("/opt/bars/bars-cli"));
    assert!(instruction.mcp_config.contains("/opt/bars/bars-mcp"));
    window.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn another_docs_root_is_written_and_indexed() {
    let window = Window::new();
    let root = window.project("alpha", "documentation");
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
    let config = WorkerConfig::new(home.path().to_path_buf());
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
