//! `wt` у инструментов MCP (`docs/features/projects/worktrees.md`, «MCP»): параметр доходит до
//! воркера в `Target`, пометка «граф и аудиты — у основного checkout» стоит над ответом аудита,
//! неизвестный `wt` — ошибка воркера, описание параметра — на английском. Воркер подменён
//! записывающим: он отвечает минимально верными ответами. Тесты не запускались: сборка
//! запрещена в этом шаге.
use super::*;
use bars_protocol::{Call, ErrorKind, Handler, RpcError, SessionInfoParams};
use std::sync::Mutex;

/// Что воркер увидел: `(метод, project, wt)` и параметры `session_info`.
#[derive(Default)]
struct Recorder {
    targets: Mutex<Vec<(String, Option<String>, Option<String>)>>,
    sessions: Mutex<Vec<SessionInfoParams>>,
}

impl Recorder {
    fn seen(&self, method: &str, target: &Target) {
        self.targets.lock().unwrap().push((
            method.to_string(),
            target.project.clone(),
            target.wt.clone(),
        ));
    }
}

impl Handler for Recorder {
    async fn handle_observed(
        &self,
        _session: &str,
        client: ClientKind,
        call: Call,
        _context: Option<Value>,
    ) -> Result<Value, RpcError> {
        self.handle(client, call).await
    }

    async fn handle(&self, _client: ClientKind, call: Call) -> Result<Value, RpcError> {
        match call {
            Call::Read(p) => {
                self.seen("read", &p.target);
                Ok(json!({"title": "Документ", "text": "текст worktree"}))
            }
            Call::Reindex(p) => {
                self.seen("reindex", &p.target);
                Ok(json!({"accepted": true}))
            }
            Call::SessionInfo(p) => {
                self.sessions.lock().unwrap().push(p.clone());
                match p.wt.as_deref() {
                    Some("nope") => Err(RpcError::bars(
                        ErrorKind::WorktreeNotFound,
                        "У проекта «main» нет worktree «nope». Есть: «feat-x» (ветка feat/x)",
                    )),
                    Some(".") => Ok(json!({"project": {"name": "main", "modules": []}})),
                    _ => Ok(json!({"project": {
                        "name": "main",
                        "modules": [],
                        "worktree": {"name": "feat-x", "branch": "feat/x"},
                    }})),
                }
            }
            Call::AuditList(_) => Ok(json!({
                "audits": [], "reindex_required": false, "indexing": false
            })),
            Call::NewTopicsList(_) => Ok(json!({
                "enabled": true, "generation": 0, "topics": [], "store_path": "x", "losses": 0
            })),
            _ => Err(RpcError::bars(ErrorKind::ProjectNotFound, "нет проекта")),
        }
    }
}

async fn recorder_session() -> (Arc<Recorder>, Env, TempDir, tokio::task::JoinHandle<()>) {
    let home = TempDir::new().unwrap();
    let endpoint = Endpoint::named(&format!(
        "wt-{}",
        home.path().file_name().unwrap().to_string_lossy()
    ));
    let listener = bars_worker::ipc::bind(&endpoint).unwrap();
    let handler = Arc::new(Recorder::default());
    let server = {
        let handler = handler.clone();
        tokio::spawn(async move {
            let _ = bars_worker::ipc::run_server(listener, handler).await;
        })
    };
    let env = Env {
        endpoint,
        cwd: home.path().into(),
        launch: Launch::With(Arc::new(|| panic!("воркер уже запущен"))),
        call_timeout: CALL_TIMEOUT,
        model_wait: MODEL_WAIT,
        bars_home: None,
    };
    (handler, env, home, server)
}

async fn connect(env: Env) -> Pipe {
    let (client, server) = tokio::io::duplex(1 << 20);
    tokio::spawn(async move {
        let service = BarsMcp::new(env).serve(server).await.unwrap();
        let _ = service.waiting().await;
    });
    let (read, write) = tokio::io::split(client);
    let mut rpc = Rpc::new(read, write);
    rpc.initialize().await;
    rpc
}

#[tokio::test(flavor = "multi_thread")]
async fn wt_parameter_reaches_target() {
    let (handler, env, _home, server) = recorder_session().await;
    let mut mcp = connect(env).await;

    let (error, _) = mcp
        .tool(
            "bars_read",
            json!({"path": "docs/guide.md", "wt": "feat-x"}),
        )
        .await;
    assert!(!error);
    // `.` — основной checkout — уходит воркеру как есть, а не путём `cwd/.`.
    mcp.tool("bars_read", json!({"path": "docs/guide.md", "wt": "."}))
        .await;
    // Пустая строка равна отсутствию.
    mcp.tool("bars_read", json!({"path": "docs/guide.md", "wt": " "}))
        .await;
    // `bars_status refresh=true wt=…` — сверка вида: ReindexParams с тем же `wt`.
    mcp.tool("bars_status", json!({"refresh": true, "wt": "feat-x"}))
        .await;

    let seen = handler.targets.lock().unwrap().clone();
    let wts: Vec<_> = seen
        .iter()
        .map(|(method, _, wt)| (method.as_str(), wt.as_deref()))
        .collect();
    assert_eq!(
        wts,
        [
            ("read", Some("feat-x")),
            ("read", Some(".")),
            ("read", None),
            ("reindex", Some("feat-x")),
        ]
    );
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
async fn audit_with_wt_has_owner_only_note_and_unknown_wt_is_an_error() {
    let (handler, env, _home, server) = recorder_session().await;
    let mut mcp = connect(env).await;

    let (error, text) = mcp.tool("bars_audit", json!({"wt": "feat-x"})).await;
    assert!(!error, "{text}");
    assert!(
        text.starts_with("worktree `feat-x`: граф, аудиты и ссылки есть только у основного"),
        "пометка строкой над ответом: {text}"
    );
    // Основной checkout: пометки нет, проверка воркеру не нужна.
    let (_, main) = mcp.tool("bars_audit", json!({"wt": "."})).await;
    assert!(!main.contains("только у основного checkout"), "{main}");

    let (error, text) = mcp.tool("bars_audit", json!({"wt": "nope"})).await;
    assert!(error && text.contains("worktree_not_found"), "{text}");

    // Проверка цели — `session_info` с явным `wt`; `.` её не вызывает.
    let sessions = handler.sessions.lock().unwrap().clone();
    let probed: Vec<_> = sessions.iter().filter_map(|p| p.wt.as_deref()).collect();
    assert_eq!(probed, ["feat-x", "nope"]);
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
async fn wt_parameter_is_described_in_english_for_every_tool() {
    let harness = Harness::new("wt-schema");
    let root = harness.project("proj");
    harness.show_graph_tools();
    let mut mcp = harness.connect(harness.env(&root));
    mcp.initialize().await;
    let tools = mcp.request("tools/list", json!({})).await;
    for tool in tools["result"]["tools"].as_array().unwrap() {
        let name = tool["name"].as_str().unwrap();
        let wt = &tool["inputSchema"]["properties"]["wt"];
        assert!(wt.is_object(), "{name}: нет параметра wt");
        let description = wt["description"].as_str().unwrap_or_default();
        assert!(
            description.contains(".git/worktrees/<name>") || description.contains("main"),
            "{name}: описание wt: {description}"
        );
        assert!(
            !description
                .chars()
                .any(|c| ('а'..='я').contains(&c.to_ascii_lowercase())),
            "{name}: описание wt — на английском (НФТ-4): {description}"
        );
    }
}
