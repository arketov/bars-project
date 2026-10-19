//! Интеграционные тесты `bars-mcp`: сырой JSON-RPC по stdio-подобному каналу, настоящий воркер
//! (канал, реестр, очередь, индекс) во временных папках, подделка embed-модели, запуск воркера
//! «по требованию» из самого прокси.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use bars_mcp::{BarsMcp, CALL_TIMEOUT, Env, INSTRUCTIONS, Launch, MODEL_WAIT};
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
                assert!(
                    config.install_env.is_none(),
                    "MCP tests must not access client profiles"
                );
                let (endpoint, provider) = (endpoint.clone(), Arc::clone(&provider));
                handle.spawn(async move { serve(config, &endpoint, provider).await });
                Ok(())
            })),
            call_timeout: CALL_TIMEOUT,
            model_wait: MODEL_WAIT,
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
            "bars_context",
            "bars_explore",
            "bars_graph",
            "bars_impact",
            "bars_outline",
            "bars_paths",
            "bars_projects",
            "bars_rag_search",
            "bars_read",
            "bars_reindex",
            "bars_relations",
            "bars_status"
        ]
    );
    assert_eq!(
        harness.launches.load(Ordering::SeqCst),
        0,
        "воркер не запускался"
    );
}

#[tokio::test(flavor="multi_thread")]
async fn all_graph_tools_and_explicit_search_modes_use_real_worker_dtos() {
    let harness=Harness::new("graph-tools");let root=harness.project("graph");
    fs::write(root.join("docs/requirements.md"),"# Требования\n\nТ-1 зависит от Т-2.\n\nТ-2 зависит от Т-3.").unwrap();
    let mut mcp=harness.connect(harness.env(&root));mcp.initialize().await;
    assert!(!mcp.tool("bars_projects",json!({})).await.0);harness.register(&root).await;
    wait_status(&mut mcp,"автоматический граф",|s|indexed(s)&&s["graph"]["state"]=="partial").await;
    let(error,text)=mcp.tool("bars_graph",json!({"view":"mixed","format":"json"})).await;assert!(!error,"{text}");
    let graph:Value=serde_json::from_str(&text).unwrap();
    let root=graph["nodes"].as_array().unwrap().iter().find(|n|n["label"]=="Т-3").unwrap()["reference"].clone();
    for(tool,args)in [
        ("bars_relations",json!({"query":"Т-1","format":"json"})),
        ("bars_paths",json!({"from":{"label":"Т-1"},"to":{"label":"Т-3"},"format":"json"})),
        ("bars_impact",json!({"roots":[root],"format":"json"})),
        ("bars_context",json!({"query":"Т-1 зависит","format":"json"})),
        ("bars_explore",json!({"whole_scope":true,"format":"json"})),
    ]{
        let(error,text)=mcp.tool(tool,args).await;assert!(!error,"{tool}: {text}");let out:Value=serde_json::from_str(&text).unwrap();
        assert!(out["items"].as_array().is_some_and(|items|!items.is_empty()),"{tool}: {text}");assert!(out["used_bytes"].as_u64().unwrap()<=out["applied"]["max_bytes"].as_u64().unwrap());
    }
    for mode in ["hybrid","mix","graph"]{let(error,text)=mcp.tool("bars_rag_search",json!({"query":"Т-1 зависит","mode":mode,"format":"json"})).await;assert!(!error,"{mode}: {text}");let out:Value=serde_json::from_str(&text).unwrap();assert_eq!(out["requested_mode"],mode);}
    let(error,text)=mcp.tool("bars_rag_search",json!({"query":"Т-1 зависит","mode":"mix","include_related":false,"format":"json"})).await;
    assert!(!error,"{text}"); let out:Value=serde_json::from_str(&text).unwrap();
    assert_eq!(out["applied_mode"],"hybrid"); assert!(out["related"].is_null());
    let(error,_)=mcp.tool("bars_rag_search",json!({"query":"Т-1 зависит","include_related":false})).await;
    assert!(error,"параметр mix не игнорируется в старом вызове");
    let tools=mcp.request("tools/list",json!({})).await;assert!(!tools.to_string().contains("graph_build"));assert!(!tools.to_string().contains("graph_cancel"));
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
            "=== OUTLINE docs/guide.md: headings 3 ===",
            "8 ## Индексация",
            "12 ### Слежение",
            "16 ## Поиск",
            "=== END OUTLINE ==="
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
        document.starts_with("=== DOCUMENT guide.md ===\ntitle: Руководство\n"),
        "{document}"
    );
    assert!(document.ends_with("=== END DOCUMENT ==="), "{document}");
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
    let rows = bars_worker::mcp_statistics::snapshot(harness.home.path()).unwrap();
    let tools: std::collections::BTreeSet<_> = rows.iter().map(|r| r.tool.as_str()).collect();
    assert_eq!(tools, ["bars_outline", "bars_projects", "bars_rag_search", "bars_read", "bars_reindex", "bars_status"].into_iter().collect());
    assert_eq!(rows.iter().map(|r| r.session.as_str()).collect::<std::collections::HashSet<_>>().len(), 1);
    assert!(rows.iter().any(|r| r.errors > 0));
    let history=bars_worker::mcp_history::History::new(harness.home.path().into());
    let page=history.page(&bars_worker::mcp_history::Filter{limit:Some(100),..Default::default()}).unwrap();
    assert_eq!(page.summary["calls"].as_u64().unwrap(),rows.iter().map(|r|r.calls as u64).sum::<u64>());
    for tool in ["bars_rag_search","bars_read","bars_outline","bars_projects","bars_status","bars_reindex"] {
        let a=page.records.iter().find(|a|a.tool==tool && a.outcome=="success").unwrap();
        assert!(a.duration_ms.is_some());assert!(a.finished.is_some());
        assert!(!a.incomplete.iter().any(|s|s=="original_mcp_input_unavailable"));
        assert!(a.requested.get("format").is_some());
        assert!(a.result.get("unit").is_some());
        if matches!(tool,"bars_status"|"bars_reindex"){assert_eq!(a.result["unit"],"not_applicable");assert!(a.result["count"].is_null());}
        if tool=="bars_reindex" {assert!(a.applied["operation_id"].is_number());assert!(a.applied["merged"].is_boolean());}
    }
    let search=page.records.iter().find(|a|a.tool=="bars_rag_search" && a.outcome=="success").unwrap();
    assert!(search.applied["search"]["results"].is_number());
    assert!(page.records.iter().any(|a|a.outcome=="error" && a.error["kind"]=="project_not_found"));
}

#[tokio::test(flavor="multi_thread")]
async fn history_preserves_original_missing_null_zero_and_excludes_payloads() {
    let harness=Harness::new("history-original");let root=harness.project("p");
    let mut mcp=harness.connect(harness.env(&root));mcp.default_format=None;mcp.initialize().await;
    assert!(!mcp.tool("bars_projects",json!({})).await.0);
    assert!(!mcp.tool("bars_projects",json!({"format":null,"unknown":"SECRET"})).await.0);
    harness.register(&root).await;
    wait_status(&mut mcp,"индекс для истории",indexed).await;
    fs::write(root.join("docs/empty.md"),"").unwrap();
    assert!(!mcp.tool("bars_read",json!({"path":"empty.md","format":"json"})).await.0);
    assert!(!mcp.tool("bars_outline",json!({"path":"guide.md","levels":[6]})).await.0);
    assert!(mcp.tool("bars_read",json!({"path":"missing.md"})).await.0);
    assert!(mcp.tool("bars_read",json!({"path":17})).await.0);
    fs::write(harness.home.path().join("settings.toml"),"[search]\nresults=2\ncandidates=5\nmax_results=5\nmax_candidates=10\n").unwrap();
    assert!(!mcp.tool("bars_rag_search",json!({"query":"первая проверка умолчаний","limit":null})).await.0);
    fs::write(harness.home.path().join("settings.toml"),"[search]\nresults=4\ncandidates=7\nmax_results=5\nmax_candidates=10\n").unwrap();
    assert!(!mcp.tool("bars_rag_search",json!({"query":"вторая проверка умолчаний"})).await.0);
    assert!(!mcp.tool("bars_rag_search",json!({"query":"проверка ограничения","limit":999,"candidates":99999})).await.0);
    assert!(!mcp.tool("bars_rag_search",json!({"query":"zzz18427 отсутствующий термин","min_score":1.0})).await.0);
    let history=bars_worker::mcp_history::History::new(harness.home.path().into());
    let page=history.page(&bars_worker::mcp_history::Filter{limit:Some(100),..Default::default()}).unwrap();
    let projects:Vec<_>=page.records.iter().filter(|a|a.tool=="bars_projects").collect();
    assert_eq!(projects.len(),2);assert_eq!(projects[0].requested,json!({"format":null}));assert_eq!(projects[1].requested,json!({}));
    assert_eq!(projects[1].result["count"],0);
    assert!(page.records.iter().any(|a|a.tool=="bars_read" && a.outcome=="success" && a.result["count"]==0 && a.result["unit"]=="lines"));
    assert!(page.records.iter().any(|a|a.tool=="bars_outline" && a.outcome=="success" && a.result["count"]==0 && a.result["unit"]=="headings"));
    let first=page.records.iter().find(|a|a.requested["query"]=="первая проверка умолчаний").unwrap();
    let second=page.records.iter().find(|a|a.requested["query"]=="вторая проверка умолчаний").unwrap();
    assert_eq!(first.applied["search"]["results"],2);assert_eq!(second.applied["search"]["results"],4);
    assert!(first.requested.get("limit").is_some_and(Value::is_null));assert!(second.requested.get("limit").is_none());
    let clamped=page.records.iter().find(|a|a.requested["query"]=="проверка ограничения").unwrap();
    assert_eq!(clamped.applied["search"]["results"],5);assert_eq!(clamped.applied["search"]["candidates"],10);assert_eq!(clamped.applied["sources"]["limit"],"clamped");
    let empty=page.records.iter().find(|a|a.requested["query"]=="zzz18427 отсутствующий термин").unwrap();assert_eq!(empty.result["count"],0);assert_eq!(empty.outcome,"success");
    let json=serde_json::to_string(&page).unwrap();assert!(!json.contains("SECRET"));assert!(!json.contains("Уникальная заметка"));
}

#[tokio::test(flavor="multi_thread")]
async fn history_parallel_connections_repeat_and_collection_controls() {
    let harness=Harness::new("history-parallel");let p=harness.project("p");let q=harness.project("q");
    let mut a=harness.connect(harness.env(&p));a.initialize().await;a.tool("bars_projects",json!({})).await;
    harness.register(&p).await;harness.register(&q).await;
    let mut b=harness.connect(harness.env(&q));b.initialize().await;
    let (x,y)=tokio::join!(a.tool("bars_read",json!({"path":"guide.md"})),b.tool("bars_read",json!({"path":"guide.md"})));
    assert!(!x.0 && !y.0);assert!(!a.tool("bars_read",json!({"path":"guide.md"})).await.0);
    let history=bars_worker::mcp_history::History::new(harness.home.path().into());
    let page=history.page(&bars_worker::mcp_history::Filter{tool:Some("bars_read".into()),..Default::default()}).unwrap();
    assert_eq!(page.records.len(),3);assert_eq!(page.records.iter().map(|a|&a.session).collect::<std::collections::HashSet<_>>().len(),2);
    assert_eq!(page.records.iter().map(|a|&a.project).collect::<std::collections::HashSet<_>>().len(),2);
    let ids:std::collections::HashSet<_>=page.records.iter().map(|a|a.id).collect();assert_eq!(ids.len(),3);
    history.clear(true).unwrap();assert!(!a.tool("bars_read",json!({"path":"guide.md"})).await.0);
    let page=history.page(&Default::default()).unwrap();assert_eq!(page.records.len(),1);assert!(!ids.contains(&page.records[0].id));
    history.set_enabled(false).unwrap();assert!(!b.tool("bars_read",json!({"path":"guide.md"})).await.0);assert_eq!(history.page(&Default::default()).unwrap().records.len(),1);
    history.set_enabled(true).unwrap();assert!(!b.tool("bars_read",json!({"path":"guide.md"})).await.0);assert_eq!(history.page(&Default::default()).unwrap().records.len(),2);
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
        // Пока второй проект не проиндексирован, воркер отвечает не JSON-ом, а
        // текстом ошибки: это не провал, а повод подождать.
        let parsed = serde_json::from_str::<Value>(&found);
        if let Ok(parsed) = &parsed
            && parsed["results"][0]["path"] == "docs/other.md"
        {
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
    let rows = bars_worker::mcp_statistics::snapshot(harness.home.path()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].calls, rows[0].errors), (2, 0));
    assert_eq!(rows[0].tool, "bars_projects");
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
    assert!(status.starts_with("=== INDEX STATUS ==="), "{status}");
    assert!(status.contains("\nstate: ready\n"), "{status}");

    let (_, projects) = mcp.tool("bars_projects", json!({})).await;
    assert!(projects.starts_with("=== PROJECTS: 1 ==="), "{projects}");
    assert!(projects.contains("=== PROJECT 1 of 1 ==="), "{projects}");
    assert!(projects.contains("api (1)"), "{projects}");

    let (is_error, found) = mcp
        .tool(
            "bars_rag_search",
            json!({ "query": "индексация документов" }),
        )
        .await;
    assert!(!is_error, "{found}");
    assert!(found.starts_with("=== SEARCH: results"), "{found}");
    assert!(found.contains("=== RESULT 1 of "), "{found}");
    assert!(found.contains("\npath: docs/guide.md\n"), "{found}");
    assert!(found.contains("\n--- text ---\n"), "{found}");
    assert!(found.contains("=== END RESULT 1 ==="), "{found}");
    assert!(found.ends_with("=== END SEARCH ==="), "{found}");

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
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 12);

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
