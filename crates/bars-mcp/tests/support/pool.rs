//! Контрольные воркеры: настоящий IPC и MCP JSON-RPC, без моделей и временных гонок поиска.
use super::*;
use bars_protocol::{Call, ErrorKind, Handler, RpcError};
use tokio::sync::{Semaphore, mpsc};

struct Controlled {
    started: mpsc::UnboundedSender<(String, String)>,
    release: Semaphore,
}

impl Handler for Controlled {
    async fn handle_observed(
        &self,
        session: &str,
        _client: ClientKind,
        call: Call,
        _context: Option<Value>,
    ) -> Result<Value, RpcError> {
        let label = match &call {
            Call::Read(p) => p.path.clone(),
            Call::Search(p) => p.query.clone(),
            Call::Reindex(_) => "write".into(),
            _ => return self.handle(ClientKind::Mcp, call).await,
        };
        self.started.send((session.into(), label.clone())).unwrap();
        if label.starts_with("hold") {
            self.release.acquire().await.unwrap().forget();
        }
        if label == "loading" {
            return Err(RpcError::bars(ErrorKind::ModelLoading, "загрузка"));
        }
        if label == "write" || label == "bad" {
            return Ok(json!({"invalid":"response"}));
        }
        if matches!(call, Call::Search(_)) {
            return Err(RpcError::bars(ErrorKind::ProjectNotFound, label));
        }
        Ok(json!({"title":label,"text":label}))
    }

    async fn handle(&self, _client: ClientKind, call: Call) -> Result<Value, RpcError> {
        match call {
            Call::SessionInfo(_) => Ok(json!({"project":null})),
            Call::Projects(_) => Ok(json!({"projects":[]})),
            _ => Err(RpcError::bars(ErrorKind::ProjectNotFound, "нет проекта")),
        }
    }
}

struct Fixture {
    _home: TempDir,
    handler: Arc<Controlled>,
    started: mpsc::UnboundedReceiver<(String, String)>,
    server: tokio::task::JoinHandle<std::io::Result<()>>,
    env: Env,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl Fixture {
    fn new(timeout: Duration) -> Self {
        let home = TempDir::new().unwrap();
        let endpoint = Endpoint::named(&format!(
            "pool-{}",
            home.path().file_name().unwrap().to_string_lossy()
        ));
        let listener = bars_worker::ipc::bind(&endpoint).unwrap();
        let (send, started) = mpsc::unbounded_channel();
        let handler = Arc::new(Controlled {
            started: send,
            release: Semaphore::new(0),
        });
        let server = tokio::spawn(bars_worker::ipc::run_server(listener, handler.clone()));
        let env = Env {
            endpoint,
            cwd: home.path().into(),
            launch: Launch::With(Arc::new(|| panic!("воркер уже запущен"))),
            call_timeout: timeout,
            model_wait: Duration::from_secs(60),
            bars_home: None,
        };
        Self {
            _home: home,
            handler,
            started,
            server,
            env,
        }
    }

    async fn connect(&self) -> Pipe {
        let (client, server) = tokio::io::duplex(1 << 20);
        let env = self.env.clone();
        tokio::spawn(async move {
            let service = BarsMcp::new(env).serve(server).await.unwrap();
            let _ = service.waiting().await;
        });
        let (read, write) = tokio::io::split(client);
        let mut rpc = Rpc::new(read, write);
        rpc.initialize().await;
        rpc
    }

    async fn started(&mut self) -> (String, String) {
        tokio::time::timeout(Duration::from_secs(5), self.started.recv())
            .await
            .unwrap()
            .unwrap()
    }
}

async fn send_tool(rpc: &mut Pipe, id: u64, name: &str, arguments: Value) {
    rpc.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call", "params":{"name":name,"arguments":arguments}})).await;
}

async fn response(rpc: &mut Pipe) -> Value {
    loop {
        let mut line = String::new();
        assert!(
            tokio::time::timeout(Duration::from_secs(5), rpc.reader.read_line(&mut line))
                .await
                .unwrap()
                .unwrap()
                > 0
        );
        let value: Value = serde_json::from_str(&line).unwrap();
        if value.get("id").is_some() {
            return value;
        }
    }
}

async fn cancel(rpc: &mut Pipe, id: u64) {
    rpc.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled", "params":{"requestId":id,"reason":"test"}})).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn pool_read_finishes_while_search_is_blocked_and_reuses_healthy_channel() {
    let mut f = Fixture::new(Duration::from_secs(10));
    let mut rpc = f.connect().await;
    send_tool(
        &mut rpc,
        100,
        "bars_search",
        json!({"query":"hold-search"}),
    )
    .await;
    let (search_session, _) = f.started().await;
    let read_started = Instant::now();
    send_tool(&mut rpc, 101, "bars_read", json!({"path":"quick"})).await;
    let (read_session, _) = f.started().await;
    assert_ne!(read_session, search_session);
    let quick = response(&mut rpc).await;
    assert_eq!(quick["id"], 101, "{quick}");
    assert_ne!(quick["result"]["isError"], true);
    eprintln!(
        "pool: short read while search blocked: {:?}",
        read_started.elapsed()
    );
    send_tool(&mut rpc, 102, "bars_read", json!({"path":"again"})).await;
    assert_eq!(f.started().await.0, read_session);
    assert_eq!(response(&mut rpc).await["id"], 102);
    f.handler.release.add_permits(1);
    assert_eq!(response(&mut rpc).await["id"], 100);
}

#[tokio::test(flavor = "multi_thread")]
async fn pool_bounds_four_channels_and_cancels_active_and_queued_calls() {
    let mut f = Fixture::new(Duration::from_secs(10));
    let mut rpc = f.connect().await;
    let mut sessions = std::collections::HashSet::new();
    for id in 100..104 {
        send_tool(
            &mut rpc,
            id,
            "bars_read",
            json!({"path":format!("hold-{id}")}),
        )
        .await;
        sessions.insert(f.started().await.0);
    }
    assert_eq!(sessions.len(), 4);
    send_tool(&mut rpc, 104, "bars_read", json!({"path":"queued-cancel"})).await;
    // Тот же поток JSON-RPC: отмена обработана до освобождения занятого места.
    cancel(&mut rpc, 104).await;
    // rmcp подавляет ответ на отменённый запрос; ping подтверждает обработку уведомления.
    assert!(rpc.request("ping", json!({})).await["result"].is_object());
    send_tool(&mut rpc, 105, "bars_read", json!({"path":"quick"})).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(100), f.started.recv())
            .await
            .is_err()
    );
    cancel(&mut rpc, 100).await;
    let (fresh, label) = f.started().await;
    assert_eq!(label, "quick");
    assert!(
        !sessions.contains(&fresh),
        "отменённый канал вернулся в пул"
    );
    assert_eq!(response(&mut rpc).await["id"], 105);
    f.handler.release.add_permits(4);
    for _ in 0..3 {
        response(&mut rpc).await;
    }
    assert!(
        f.started.try_recv().is_err(),
        "отменённый запрос попал в воркер"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn pool_total_timeout_discards_channel_and_bounds_model_polling() {
    let mut f = Fixture::new(Duration::from_millis(250));
    let mut rpc = f.connect().await;
    send_tool(&mut rpc, 100, "bars_read", json!({"path":"hold-late"})).await;
    let (old, _) = f.started().await;
    let reply = response(&mut rpc).await;
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    assert!(reply.to_string().contains("общий срок"));
    f.handler.release.add_permits(1);
    send_tool(&mut rpc, 101, "bars_read", json!({"path":"fresh"})).await;
    assert_ne!(f.started().await.0, old);
    let reply = response(&mut rpc).await;
    assert_eq!(reply["id"], 101);
    assert!(reply.to_string().contains("fresh"));
    let started = Instant::now();
    send_tool(&mut rpc, 102, "bars_read", json!({"path":"loading"})).await;
    f.started().await;
    assert_eq!(response(&mut rpc).await["result"]["isError"], true);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "ожидание модели обошло бюджет"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn pool_cancel_during_model_wait_prevents_retry_and_keeps_other_calls_alive() {
    let mut f = Fixture::new(Duration::from_secs(10));
    let mut rpc = f.connect().await;
    send_tool(&mut rpc, 100, "bars_read", json!({"path":"loading"})).await;
    f.started().await;
    cancel(&mut rpc, 100).await;
    assert!(rpc.request("ping", json!({})).await["result"].is_object());
    send_tool(&mut rpc, 101, "bars_read", json!({"path":"fresh"})).await;
    assert_eq!(f.started().await.1, "fresh");
    assert_eq!(response(&mut rpc).await["id"], 101);
    assert!(
        tokio::time::timeout(Duration::from_millis(1200), f.started.recv())
            .await
            .is_err()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn pool_read_annotations_include_graph_and_exclude_mutations() {
    let f = Fixture::new(Duration::from_secs(10));
    let mut env = f.env.clone();
    fs::write(
        f._home.path().join("settings.toml"),
        "[mcp]\ngraph_tools=true\n",
    )
    .unwrap();
    env.bars_home = Some(f._home.path().into());
    let server = BarsMcp::new(env);
    let tools = rmcp::ServerHandler::get_tool(&server, "bars_graph").unwrap();
    assert_eq!(tools.annotations.unwrap().read_only_hint, Some(true));
    let mut rpc = f.connect().await;
    let list = rpc.request("tools/list", json!({})).await;
    for tool in list["result"]["tools"].as_array().unwrap() {
        let name = tool["name"].as_str().unwrap();
        let expected = !matches!(name, "bars_audit");
        assert_eq!(tool["annotations"]["readOnlyHint"], expected, "{name}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn pool_bad_read_retries_once_but_ambiguous_write_never_replays() {
    let mut f = Fixture::new(Duration::from_secs(10));
    let mut rpc = f.connect().await;
    assert!(rpc.tool("bars_read", json!({"path":"bad"})).await.0);
    let first = f.started().await;
    let second = f.started().await;
    assert_ne!(first.0, second.0);
    // Запись теперь — refresh у сводки: неопределённый результат не повторяется,
    // ошибка остаётся частью сводки, а не сбоем вызова.
    let (error, body) = rpc.tool("bars_status", json!({"refresh":true})).await;
    assert!(!error && body.contains("=== REFRESH ===\nerror:"), "{body}");
    assert_eq!(f.started().await.1, "write");
    assert!(
        f.started.try_recv().is_err(),
        "запись повторилась после неопределённого результата"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn pool_queue_time_is_part_of_total_deadline() {
    let mut f = Fixture::new(Duration::from_secs(1));
    let mut rpc = f.connect().await;
    for id in 100..104 {
        send_tool(
            &mut rpc,
            id,
            "bars_read",
            json!({"path":format!("hold-{id}")}),
        )
        .await;
        f.started().await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let start = Instant::now();
    send_tool(&mut rpc, 104, "bars_read", json!({"path":"hold-queued"})).await;
    loop {
        let reply = response(&mut rpc).await;
        if reply["id"] == 104 {
            assert_eq!(reply["result"]["isError"], true);
            break;
        }
    }
    assert!(
        start.elapsed() < Duration::from_millis(1600),
        "очередь получила отдельный срок: {:?}",
        start.elapsed()
    );
    f.handler.release.add_permits(5);
}

#[tokio::test(flavor = "multi_thread")]
async fn pool_connect_and_worker_launch_are_inside_total_deadline() {
    let mut f = Fixture::new(Duration::from_millis(150));
    // Запуск не создаёт канал: весь connect/hello обязан прерываться общим сроком.
    f.env.endpoint = Endpoint::named(&format!(
        "absent-{}",
        f._home.path().file_name().unwrap().to_string_lossy()
    ));
    f.env.launch = Launch::With(Arc::new(|| Ok(())));
    let mut absent = f.connect().await;
    let start = Instant::now();
    let (error, body) = absent.tool("bars_read", json!({"path":"missing"})).await;
    assert!(error && body.contains("общий срок"), "{body}");
    assert!(start.elapsed() < Duration::from_secs(1));
}
