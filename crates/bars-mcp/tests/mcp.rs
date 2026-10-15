//! Интеграционные тесты `bars-mcp`: сырой JSON-RPC по stdio-подобному каналу, настоящий воркер
//! (канал, реестр, очередь, индекс) во временных папках, подделка embed-модели, запуск воркера
//! «по требованию» из самого прокси.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use bars_mcp::{BarsMcp, CALL_TIMEOUT, Env, INSTRUCTIONS, Launch};
use bars_protocol::{Client, ClientKind, Endpoint, RegisterParams};
use bars_worker::serve::serve;
use bars_worker::worker::WorkerConfig;
use bars_worker::worker::models::fake::FakeProvider;
use rmcp::ServiceExt;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf};

const GUIDE: &str = "---\ntitle: Руководство\n---\n# Руководство\n\nВведение.\n\n## Индексация\n\n\
    Индексация документов идёт в очереди воркера.\n\n### Слежение\n\nСлежение за файлами.\n\n\
    ## Поиск\n\nПоиск работает по индексу.\n";

/// Сырой MCP-клиент: по строке JSON-RPC на сообщение.
struct Rpc<R, W> {
    reader: BufReader<R>,
    writer: W,
    next_id: u64,
    /// Формат, который `tool` подставляет, если его нет в аргументах.
    default_format: Option<&'static str>,
}

impl<R: tokio::io::AsyncRead + Unpin, W: tokio::io::AsyncWrite + Unpin> Rpc<R, W> {
    fn new(reader: R, writer: W) -> Self {
        Self {
            reader: BufReader::new(reader),
            writer,
            next_id: 1,
            default_format: Some("json"),
        }
    }

    async fn send(&mut self, message: Value) {
        let mut line = message.to_string();
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await.unwrap();
        self.writer.flush().await.unwrap();
    }

    async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await;
        loop {
            let mut line = String::new();
            let read =
                tokio::time::timeout(Duration::from_secs(60), self.reader.read_line(&mut line))
                    .await
                    .expect("сервер не ответил за 60 с")
                    .unwrap();
            assert!(read > 0, "сервер закрыл вывод, не ответив на {method}");
            let message: Value = serde_json::from_str(&line).expect("сервер пишет JSON по строке");
            if message.get("id") == Some(&json!(id)) {
                return message;
            }
        }
    }

    async fn initialize(&mut self) -> Value {
        let response = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": { "name": "bars-test", "version": "0.0.0" }
                }),
            )
            .await;
        self.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .await;
        response
    }

    /// Вызывает инструмент: `(is_error, текст)`.
    async fn tool(&mut self, name: &str, mut arguments: Value) -> (bool, String) {
        if let (Some(format), Some(object)) = (self.default_format, arguments.as_object_mut()) {
            object.entry("format").or_insert(json!(format));
        }
        let response = self
            .request(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            )
            .await;
        let result = &response["result"];
        assert!(
            result.is_object(),
            "протокольная ошибка вместо ответа: {response}"
        );
        let text = result["content"]
            .as_array()
            .expect("content")
            .iter()
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        (result["isError"] == json!(true), text)
    }
}

type Pipe = Rpc<ReadHalf<DuplexStream>, WriteHalf<DuplexStream>>;

struct Harness {
    home: TempDir,
    projects: TempDir,
    endpoint: Endpoint,
    provider: Arc<FakeProvider>,
    launches: Arc<AtomicUsize>,
}

impl Harness {
    fn new(name: &str) -> Self {
        Self {
            home: TempDir::new().unwrap(),
            projects: TempDir::new().unwrap(),
            endpoint: Endpoint::named(&format!("mcp-{name}")),
            provider: FakeProvider::new("m1"),
            launches: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn project(&self, name: &str) -> PathBuf {
        let root = self.projects.path().join(name);
        fs::create_dir_all(root.join("docs/api")).unwrap();
        fs::write(root.join("docs/guide.md"), GUIDE).unwrap();
        fs::write(
            root.join("docs/api/rest.md"),
            "# REST\n\nМетоды протокола.\n",
        )
        .unwrap();
        root
    }

    /// Воркер ещё не запущен: его запустит первый вызов инструмента через `Launch`.
    fn env(&self, cwd: &Path) -> Env {
        let handle = tokio::runtime::Handle::current();
        let (home, endpoint, provider, launches) = (
            self.home.path().to_path_buf(),
            self.endpoint.clone(),
            Arc::clone(&self.provider),
            Arc::clone(&self.launches),
        );
        Env {
            endpoint: self.endpoint.clone(),
            cwd: cwd.to_path_buf(),
            launch: Launch::With(Arc::new(move || {
                launches.fetch_add(1, Ordering::SeqCst);
                let config = WorkerConfig::new(home.clone());
                let (endpoint, provider) = (endpoint.clone(), Arc::clone(&provider));
                handle.spawn(async move { serve(config, &endpoint, provider).await });
                Ok(())
            })),
            call_timeout: CALL_TIMEOUT,
        }
    }

    /// MCP-сервер в памяти и сырой клиент к нему.
    fn connect(&self, env: Env) -> Pipe {
        let (client_side, server_side) = tokio::io::duplex(1 << 20);
        tokio::spawn(async move {
            let service = BarsMcp::new(env).serve(server_side).await.unwrap();
            let _ = service.waiting().await;
        });
        let (read, write) = tokio::io::split(client_side);
        Rpc::new(read, write)
    }

    /// Регистрирует проект как человек: у MCP метода `register` нет.
    async fn register(&self, root: &Path) {
        let mut cli = Client::connect(&self.endpoint, ClientKind::Cli)
            .await
            .unwrap();
        cli.call(&RegisterParams {
            path: root.display().to_string(),
        })
        .await
        .unwrap();
    }
}

async fn wait_status(mcp: &mut Pipe, what: &str, condition: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let (is_error, text) = mcp.tool("bars_status", json!({ "format": "json" })).await;
        if !is_error {
            let status: Value = serde_json::from_str(&text).unwrap();
            if condition(&status) {
                return status;
            }
        }
        assert!(Instant::now() < deadline, "не дождались: {what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn indexed(status: &Value) -> bool {
    status["state"] == "ready" && !status["updated_at"].is_null() && status["pending"] == 0
}

#[tokio::test(flavor = "multi_thread")]
async fn initialize_and_tool_list_do_not_touch_the_worker() {
    let harness = Harness::new("init");
    let root = harness.project("proj");
    let mut mcp = harness.connect(harness.env(&root));

    let init = mcp.initialize().await;
    let instructions = init["result"]["instructions"]
        .as_str()
        .expect("instructions в ответе на initialize (Т-27)");
    assert_eq!(instructions, INSTRUCTIONS);
    assert!(
        instructions.chars().count() <= 2000,
        "Т-27: не длиннее 2 000 знаков"
    );

    let tools = mcp.request("tools/list", json!({})).await;
    let mut names: Vec<_> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_string())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "bars_outline",
            "bars_projects",
            "bars_rag_search",
            "bars_read",
            "bars_reindex",
            "bars_status"
        ]
    );
    assert_eq!(
        harness.launches.load(Ordering::SeqCst),
        0,
        "воркер не запускался"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn first_tool_call_starts_the_worker_and_all_six_tools_answer() {
    let harness = Harness::new("scenario");
    let root = harness.project("proj");
    let mut mcp = harness.connect(harness.env(&root));
    mcp.initialize().await;

    // Т-2: воркер не запущен, первый вызов запускает его и возвращает результат.
    let (is_error, projects) = mcp.tool("bars_projects", json!({})).await;
    assert!(!is_error, "{projects}");
    assert_eq!(harness.launches.load(Ordering::SeqCst), 1);
    assert_eq!(
        serde_json::from_str::<Value>(&projects).unwrap()["projects"],
        json!([])
    );

    // Проект из cwd сессии не зарегистрирован: понятная ошибка с подсказкой, а не сбой протокола.
    let (is_error, text) = mcp.tool("bars_status", json!({})).await;
    assert!(is_error);
    assert!(text.contains("project_not_found"), "{text}");
    assert!(text.contains("register"), "{text}");

    harness.register(&root).await;
    let status = wait_status(&mut mcp, "первая индексация", indexed).await;
    assert!(status["updated_at"].is_string(), "{status}");
    assert_eq!(
        harness.launches.load(Ordering::SeqCst),
        1,
        "второй воркер не нужен"
    );

    let (_, projects) = mcp.tool("bars_projects", json!({})).await;
    let projects: Value = serde_json::from_str(&projects).unwrap();
    assert_eq!(projects["projects"][0]["docs_root"], "docs");
    assert!(
        projects["projects"][0]["modules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|module| module["name"] == "api")
    );

    let (is_error, found) = mcp
        .tool(
            "bars_rag_search",
            json!({ "query": "индексация документов" }),
        )
        .await;
    assert!(!is_error, "{found}");
    let found: Value = serde_json::from_str(&found).unwrap();
    assert_eq!(found["results"][0]["path"], "docs/guide.md");
    assert!(found["applied"]["results"].as_u64().unwrap() >= 1);

    // Сужение по модулю (Т-14) и ошибка параметра.
    let (is_error, narrowed) = mcp
        .tool(
            "bars_rag_search",
            json!({ "query": "протокола", "modules": ["api"], "limit": 1 }),
        )
        .await;
    assert!(!is_error, "{narrowed}");
    let narrowed: Value = serde_json::from_str(&narrowed).unwrap();
    assert!(
        narrowed["results"]
            .as_array()
            .unwrap()
            .iter()
            .all(|hit| hit["module"] == "api")
    );
    let (is_error, text) = mcp
        .tool(
            "bars_rag_search",
            json!({ "query": "x", "modules": ["нет"] }),
        )
        .await;
    assert!(is_error);
    assert!(text.contains("invalid_params"), "{text}");

    let (is_error, outline) = mcp
        .tool(
            "bars_outline",
            json!({ "path": "docs/guide.md", "levels": [2, 3], "format": "txt" }),
        )
        .await;
    assert!(!is_error, "{outline}");
    assert_eq!(
        outline.lines().collect::<Vec<_>>(),
        [
            "=== СТРУКТУРА docs/guide.md: заголовков 3 ===",
            "8 ## Индексация",
            "12 ### Слежение",
            "16 ## Поиск",
            "=== КОНЕЦ СТРУКТУРЫ ==="
        ]
    );
    let (_, under) = mcp
        .tool(
            "bars_outline",
            json!({ "path": "guide.md", "under": "Руководство/Индексация", "format": "txt" }),
        )
        .await;
    assert!(under.contains("\n12 ### Слежение\n"), "{under}");

    let (is_error, document) = mcp
        .tool("bars_read", json!({ "path": "guide.md", "format": "txt" }))
        .await;
    assert!(!is_error, "{document}");
    assert!(
        document.starts_with("=== ДОКУМЕНТ guide.md ===\ntitle: Руководство\n"),
        "{document}"
    );
    assert!(document.ends_with("=== КОНЕЦ ДОКУМЕНТА ==="), "{document}");
    assert!(document.contains("Индексация документов идёт в очереди воркера."));
    let (is_error, text) = mcp.tool("bars_read", json!({ "path": "нет.md" })).await;
    assert!(is_error);
    assert!(text.contains("file_not_found"), "{text}");

    let (is_error, reindex) = mcp.tool("bars_reindex", json!({ "full": true })).await;
    assert!(!is_error, "{reindex}");
    assert_eq!(
        serde_json::from_str::<Value>(&reindex).unwrap(),
        json!({ "accepted": true })
    );
    wait_status(&mut mcp, "полная переиндексация", indexed).await;

    // Т-15: явный проект — путь; чужой путь — project_not_found.
    let (is_error, text) = mcp
        .tool(
            "bars_status",
            json!({ "project": root.display().to_string() }),
        )
        .await;
    assert!(!is_error, "{text}");
    let (is_error, text) = mcp
        .tool(
            "bars_status",
            json!({ "project": harness.projects.path().join("other").display().to_string() }),
        )
        .await;
    assert!(is_error);
    assert!(text.contains("project_not_found"), "{text}");
}

#[tokio::test(flavor = "multi_thread")]
async fn project_is_resolved_by_session_cwd_and_explicit_project_wins() {
    let harness = Harness::new("cwd");
    let first = harness.project("first");
    let second = harness.project("second");
    fs::write(
        second.join("docs/other.md"),
        "# Особый\n\nУникальная заметка про бобров.\n",
    )
    .unwrap();

    // Сессия в подпапке первого проекта: проект находится вверх по дереву.
    let mut mcp = harness.connect(harness.env(&first.join("docs").join("api")));
    mcp.initialize().await;
    mcp.tool("bars_projects", json!({})).await;
    harness.register(&first).await;
    harness.register(&second).await;
    wait_status(&mut mcp, "индексация первого", indexed).await;

    let (_, status) = mcp.tool("bars_status", json!({})).await;
    let status: Value = serde_json::from_str(&status).unwrap();
    assert!(
        status["path"].as_str().unwrap().ends_with("first"),
        "{status}"
    );

    // Второй проект индексируется параллельно: ждём его документ.
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let (_, found) = mcp
            .tool(
                "bars_rag_search",
                json!({ "query": "бобров", "project": second.display().to_string() }),
            )
            .await;
        let found: Value = serde_json::from_str(&found).unwrap();
        if found["results"][0]["path"] == "docs/other.md" {
            break;
        }
        assert!(Instant::now() < deadline, "не нашли документ: {found}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_cannot_consent_to_reindex_after_model_change() {
    let harness = Harness::new("consent");
    let root = harness.project("proj");
    let mut mcp = harness.connect(harness.env(&root));
    mcp.initialize().await;
    mcp.tool("bars_projects", json!({})).await;
    harness.register(&root).await;
    wait_status(&mut mcp, "первая индексация", indexed).await;

    // Модель в настройках сменилась: индекс построен другой.
    harness.provider.set_model("m2");
    let mut cli = Client::connect(&harness.endpoint, ClientKind::Cli)
        .await
        .unwrap();
    let _ = cli
        .call(&bars_protocol::ReindexParams {
            target: bars_protocol::Target {
                cwd: root.display().to_string(),
                project: None,
            },
            full: false,
            accept_model_change: false,
        })
        .await;
    wait_status(&mut mcp, "состояние reindex_required", |status| {
        status["state"] == "reindex_required"
    })
    .await;

    let (is_error, text) = mcp.tool("bars_reindex", json!({ "full": true })).await;
    assert!(is_error, "{text}");
    assert!(text.contains("reindex_needs_consent"), "{text}");
    assert!(
        text.contains("пользовател"),
        "агенту сказано, что решает человек: {text}"
    );

    // Поиск в этом состоянии тоже не работает, и тоже понятным ответом.
    let (is_error, text) = mcp
        .tool("bars_rag_search", json!({ "query": "поиск" }))
        .await;
    assert!(is_error);
    assert!(text.contains("reindex_required"), "{text}");

    // Согласия не было: индекс остался прежним.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (_, status) = mcp.tool("bars_status", json!({})).await;
    let status: Value = serde_json::from_str(&status).unwrap();
    assert_eq!(status["state"], "reindex_required", "{status}");
}

#[tokio::test(flavor = "multi_thread")]
async fn unreachable_worker_is_a_readable_tool_error() {
    let harness = Harness::new("down");
    let root = harness.project("proj");
    let mut env = harness.env(&root);
    env.launch = Launch::With(Arc::new(|| {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "нет bars",
        ))
    }));
    let mut mcp = harness.connect(env);
    mcp.initialize().await;

    let (is_error, text) = mcp.tool("bars_projects", json!({})).await;
    assert!(is_error);
    assert!(text.contains("нет bars"), "{text}");
    assert!(text.contains("Повторите"), "{text}");
}

#[tokio::test(flavor = "multi_thread")]
async fn connection_to_the_worker_lives_across_calls() {
    let harness = Harness::new("reuse");
    let root = harness.project("proj");
    let mut mcp = harness.connect(harness.env(&root));
    mcp.initialize().await;
    let (is_error, _) = mcp.tool("bars_projects", json!({})).await;
    assert!(!is_error);
    let (is_error, _) = mcp.tool("bars_projects", json!({})).await;
    assert!(!is_error);
    assert_eq!(harness.launches.load(Ordering::SeqCst), 1);
}

/// Текст по умолчанию: блоки с явными границами; `format=json` — JSON ответа воркера.
#[tokio::test(flavor = "multi_thread")]
async fn default_format_is_text_with_explicit_delimiters() {
    let harness = Harness::new("txt");
    let root = harness.project("proj");
    let mut mcp = harness.connect(harness.env(&root));
    mcp.default_format = None;
    mcp.initialize().await;
    mcp.tool("bars_projects", json!({})).await;
    harness.register(&root).await;
    wait_status(&mut mcp, "первая индексация", indexed).await;

    let (_, status) = mcp.tool("bars_status", json!({ "format": "txt" })).await;
    assert!(status.starts_with("=== СОСТОЯНИЕ ИНДЕКСА ==="), "{status}");
    assert!(status.contains("\nstate: ready\n"), "{status}");

    let (_, projects) = mcp.tool("bars_projects", json!({})).await;
    assert!(projects.starts_with("=== ПРОЕКТЫ: 1 ==="), "{projects}");
    assert!(projects.contains("=== ПРОЕКТ 1 из 1 ==="), "{projects}");
    assert!(projects.contains("api (1)"), "{projects}");

    let (is_error, found) = mcp
        .tool(
            "bars_rag_search",
            json!({ "query": "индексация документов" }),
        )
        .await;
    assert!(!is_error, "{found}");
    assert!(found.starts_with("=== ПОИСК: результатов"), "{found}");
    assert!(found.contains("=== РЕЗУЛЬТАТ 1 из "), "{found}");
    assert!(found.contains("\npath: docs/guide.md\n"), "{found}");
    assert!(found.contains("\n--- текст ---\n"), "{found}");
    assert!(found.contains("=== КОНЕЦ РЕЗУЛЬТАТА 1 ==="), "{found}");
    assert!(found.ends_with("=== КОНЕЦ ПОИСКА ==="), "{found}");

    let (_, as_json) = mcp
        .tool(
            "bars_rag_search",
            json!({ "query": "индексация документов", "format": "json" }),
        )
        .await;
    assert!(serde_json::from_str::<Value>(&as_json).is_ok(), "{as_json}");

    let (_, reindex) = mcp.tool("bars_reindex", json!({})).await;
    assert!(reindex.contains("accepted: true"), "{reindex}");
}

/// НФТ-3 и Т-2 на настоящем exe: `initialize` отвечает сразу, без воркера, и процесс выходит, когда
/// клиент закрывает stdin.
#[tokio::test(flavor = "multi_thread")]
async fn real_binary_answers_initialize_without_a_worker_and_exits_on_eof() {
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_bars-mcp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("запуск bars-mcp");
    let (stdin, stdout) = (child.stdin.take().unwrap(), child.stdout.take().unwrap());
    let mut mcp = Rpc::new(stdout, stdin);

    let started = Instant::now();
    let init = mcp.initialize().await;
    assert!(init["result"]["instructions"].is_string());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "initialize ждал воркера: {:?}",
        started.elapsed()
    );
    let tools = mcp.request("tools/list", json!({})).await;
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 6);

    drop(mcp);
    let status = tokio::time::timeout(Duration::from_secs(10), child.wait())
        .await
        .expect("bars-mcp не вышел после закрытия stdin")
        .unwrap();
    assert!(status.success(), "{status}");
}

/// НФТ-3: прокси не тянет ядро и воркер — индекса и моделей в нём нет.
#[test]
fn proxy_has_no_heavy_dependencies() {
    let manifest =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    let dependencies = manifest
        .split("[dependencies]")
        .nth(1)
        .and_then(|rest| rest.split("[dev-dependencies]").next())
        .expect("секция [dependencies]");
    for heavy in ["bars-core", "bars-worker", "fastembed", "ort", "rusqlite"] {
        assert!(
            !dependencies
                .lines()
                .any(|line| line.trim_start().starts_with(heavy)),
            "НФТ-3: {heavy} не должен быть в [dependencies] bars-mcp"
        );
    }
}
