//! Тесты обработчика: настоящий канал, настоящий индекс во временной папке, подделка embed-модели.
//! Одна проверка идёт на настоящей bge-m3 из `~/.bars/models`; без модели она пропускается.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bars_protocol::{
    ActivityOutput, ActivityParams, Call, CancelOutput, Client, ClientError, ClientKind,
    DuplicateKind, DuplicatesParams, Endpoint, ErrorKind, IndexCancelParams, IndexPauseParams,
    IndexResumeParams, JobKind, JobPriority, METHOD_NOT_FOUND, OutlineParams, ProjectState,
    ProjectsParams, ReadParams, RegisterParams, ReindexParams, RpcError, SearchParams,
    SessionStartParams, SimilarParams, StatusOutput, StatusParams, Target, TreeOutput, TreeParams,
    UnregisterParams,
};
use tempfile::TempDir;

use crate::clients::InstallEnv;
use crate::serve::serve;
use crate::worker::models::fake::FakeProvider;
use crate::worker::{BuiltinProvider, ModelProvider, WorkerConfig};
mod history;

struct Harness {
    home: TempDir,
    projects: TempDir,
    endpoint: Endpoint,
}

#[tokio::test]
async fn mcp_request_starts_observation_while_cli_status_does_not() {
    let harness = Harness::start("mcp-observation", FakeProvider::new("m1")).await;
    let root = harness.project("quiet", &[("guide.md", "# Руководство\n")]);
    let mut cli = harness.client(ClientKind::Cli).await;
    cli.call(&RegisterParams {
        path: root.display().to_string(),
    })
    .await
    .unwrap();
    let params = StatusParams {
        target: target(&root),
    };
    assert!(!cli.call(&params).await.unwrap().watching);
    let mut mcp = harness.client(ClientKind::Mcp).await;
    mcp.call(&ReadParams {
        target: target(&root),
        path: "guide.md".into(),
    })
    .await
    .unwrap();
    assert!(cli.call(&params).await.unwrap().watching);
    let denied = cli
        .call(&bars_protocol::ProjectObserveParams {
            window: "fake".into(),
            project: Some(root.display().to_string()),
        })
        .await
        .unwrap_err();
    assert!(
        matches!(denied, ClientError::Rpc(error) if error.code == bars_protocol::METHOD_NOT_FOUND)
    );
}

#[tokio::test]
async fn mcp_statistics_real_ipc_sessions_projects_errors_and_cli_exclusion() {
    let harness = Harness::start("mcp-statistics", FakeProvider::new("m1")).await;
    let root = harness.project("stats", &[("guide.md", "# Руководство\n")]);
    let mut cli = harness.client(ClientKind::Cli).await;
    cli.call(&RegisterParams {
        path: root.display().to_string(),
    })
    .await
    .unwrap();
    cli.call(&ProjectsParams {}).await.unwrap();
    assert!(
        crate::mcp_statistics::snapshot(harness.home.path())
            .unwrap()
            .is_empty()
    );
    let mut a = harness.client(ClientKind::Mcp).await;
    a.call(&ReadParams {
        target: target(&root),
        path: "guide.md".into(),
    })
    .await
    .unwrap();
    let explicit = Target {
        cwd: "unrelated".into(),
        project: Some(root.display().to_string()),
    };
    a.call(&ReadParams {
        target: explicit.clone(),
        path: "absent.md".into(),
    })
    .await
    .unwrap_err();
    a.call(&StatusParams {
        target: Target {
            cwd: "unrelated".into(),
            project: None,
        },
    })
    .await
    .unwrap_err();
    a.call(&ProjectsParams {}).await.unwrap();
    let mut b = harness.client(ClientKind::Mcp).await;
    b.call(&ReadParams {
        target: explicit,
        path: "guide.md".into(),
    })
    .await
    .unwrap();
    let rows = crate::mcp_statistics::snapshot(harness.home.path()).unwrap();
    assert_eq!(rows.iter().map(|r| r.calls).sum::<i64>(), 5);
    assert_eq!(rows.iter().map(|r| r.errors).sum::<i64>(), 2);
    assert_eq!(
        rows.iter()
            .map(|r| r.session.as_str())
            .collect::<std::collections::HashSet<_>>()
            .len(),
        2
    );
    let reads: Vec<_> = rows.iter().filter(|r| r.tool == "bars_read").collect();
    assert_eq!(reads.len(), 2);
    assert!(
        reads
            .iter()
            .all(|r| r.project == root.display().to_string())
    );
    assert!(reads.iter().any(|r| r.calls == 2 && r.errors == 1));
    assert!(
        rows.iter()
            .any(|r| r.tool == "bars_status" && r.project.is_empty() && r.errors == 1)
    );
    let json = serde_json::to_string(&rows).unwrap();
    assert!(
        !json.contains("guide.md") && !json.contains("absent.md") && !json.contains("Руководство")
    );
    let path = harness.home.path().join("mcp-statistics.sqlite3");
    // Занятый writer воспроизводит отказ записи без изменения файла пользователя.
    let db = rusqlite::Connection::open(path).unwrap();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    b.call(&ProjectsParams {}).await.unwrap();
    db.execute_batch("ROLLBACK").unwrap();
}

impl Harness {
    async fn start(name: &str, provider: Arc<dyn ModelProvider>) -> Self {
        let home = TempDir::new().unwrap();
        let endpoint = Endpoint::named(&format!("handler-{name}"));
        let mut config = WorkerConfig::new(home.path().to_path_buf());
        // Клиенты живут во временной папке: настоящие ~/.claude и ~/.codex тесты не трогают.
        config.install_env = Some(InstallEnv {
            home: home.path().join("client-home"),
            mcp_exe: PathBuf::from("/opt/bars/bars-mcp"),
            cli_exe: PathBuf::from("/opt/bars/bars-cli"),
        });
        let served = endpoint.clone();
        tokio::spawn(async move { serve(config, &served, provider).await });
        Self {
            home,
            projects: TempDir::new().unwrap(),
            endpoint,
        }
    }

    async fn client(&self, kind: ClientKind) -> Client {
        Client::connect_with_retry(&self.endpoint, kind, Duration::from_secs(10))
            .await
            .unwrap()
    }

    /// Проект с документами в `docs/`: `(путь внутри docs, текст)`.
    fn project(&self, name: &str, docs: &[(&str, &str)]) -> PathBuf {
        let root = self.projects.path().join(name);
        fs::create_dir_all(root.join("docs")).unwrap();
        for (file, text) in docs {
            let path = root.join("docs").join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        root
    }
}

fn target(cwd: &Path) -> Target {
    Target {
        cwd: cwd.display().to_string(),
        project: None,
    }
}

fn search(cwd: &Path, query: &str) -> SearchParams {
    SearchParams {
        target: target(cwd),
        query: query.into(),
        modules: vec![],
        files: vec![],
        limit: None,
        candidates: None,
        min_score: None,
    }
}

async fn wait_status(
    client: &mut Client,
    cwd: &Path,
    what: &str,
    condition: impl Fn(&StatusOutput) -> bool,
) -> StatusOutput {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let status = client
            .call(&StatusParams {
                target: target(cwd),
            })
            .await
            .unwrap();
        if condition(&status) {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "не дождались: {what}: {status:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn indexed(status: &StatusOutput) -> bool {
    status.state == ProjectState::Ready && status.updated_at.is_some() && status.pending == 0
}

fn rpc<T: std::fmt::Debug>(result: Result<T, ClientError>) -> RpcError {
    match result {
        Err(ClientError::Rpc(error)) => error,
        other => panic!("ждали ошибку воркера, получили {other:?}"),
    }
}

const GUIDE: &str = "---\ntitle: Руководство\n---\n# Руководство\n\nВведение.\n\n## Индексация\n\n\
    Индексация документов идёт в очереди воркера.\n\n### Слежение\n\nСлежение за файлами.\n\n\
    ## Поиск\n\nПоиск работает по индексу.\n";

async fn ready_project(harness: &Harness, client: &mut Client) -> PathBuf {
    let root = harness.project(
        "proj",
        &[
            ("guide.md", GUIDE),
            ("api/rest.md", "# REST\n\nМетоды протокола.\n"),
        ],
    );
    let out = client
        .call(&RegisterParams {
            path: root.display().to_string(),
        })
        .await
        .unwrap();
    assert_eq!(out.project.name, "proj");
    assert_eq!(out.project.docs_root, "docs");
    wait_status(client, &root, "первая индексация", indexed).await;
    root
}

#[tokio::test]
async fn cli_works_with_a_project_end_to_end() {
    let harness = Harness::start("flow", FakeProvider::new("m1")).await;
    let mut cli = harness.client(ClientKind::Cli).await;
    let root = ready_project(&harness, &mut cli).await;
    // `cwd` в подпапке проекта находит проект вверх по дереву (Т-15).
    let cwd = root.join("docs").join("api");

    let found = cli.call(&search(&cwd, "индексация")).await.unwrap();
    let first = &found.results[0];
    assert_eq!(first.path, "docs/guide.md");
    assert_eq!(first.title, "Руководство");
    assert!(
        found
            .results
            .iter()
            .any(|hit| hit.text.contains("Индексация документов")),
        "{:?}",
        found.results
    );
    assert!(!found.indexing);
    assert_eq!(found.pending, 0);
    assert_eq!(found.applied.results, 5);

    let narrowed = cli
        .call(&SearchParams {
            modules: vec!["api".into()],
            ..search(&cwd, "методы протокола")
        })
        .await
        .unwrap();
    assert!(narrowed.results.iter().all(|hit| hit.module == "api"));

    let outline = cli
        .call(&OutlineParams {
            target: target(&cwd),
            path: "guide.md".into(),
            levels: vec![2],
            under: None,
            under_line: None,
        })
        .await
        .unwrap();
    let lines: Vec<String> = outline.headings.iter().map(|h| h.to_line()).collect();
    assert_eq!(lines, ["8 ## Индексация", "16 ## Поиск"]);

    let nested = cli
        .call(&OutlineParams {
            target: target(&cwd),
            path: "docs/guide.md".into(),
            levels: vec![],
            under: Some("Руководство/Индексация".into()),
            under_line: None,
        })
        .await
        .unwrap();
    assert_eq!(nested.headings.len(), 1);
    assert_eq!(nested.headings[0].text, "Слежение");

    let read = cli
        .call(&ReadParams {
            target: target(&cwd),
            path: "guide.md".into(),
        })
        .await
        .unwrap();
    assert_eq!(read.title, "Руководство");
    assert_eq!(read.text, GUIDE);

    let projects = cli.call(&ProjectsParams {}).await.unwrap().projects;
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].state, ProjectState::Ready);
    let modules: Vec<(&str, u32)> = projects[0]
        .modules
        .iter()
        .map(|m| (m.name.as_str(), m.documents))
        .collect();
    assert_eq!(modules, [(".", 1), ("api", 1)]);

    let status = cli
        .call(&StatusParams {
            target: target(&cwd),
        })
        .await
        .unwrap();
    assert!(!status.watching);
    assert_eq!(status.model.index.as_deref(), Some("builtin:m1"));
    assert_eq!(status.model.settings, "builtin:m1");
    assert!(status.updated_at.unwrap().ends_with('Z'));

    let removed = cli
        .call(&UnregisterParams {
            target: target(&cwd),
        })
        .await
        .unwrap();
    assert!(removed.removed);
    let error = rpc(cli.call(&search(&cwd, "индексация")).await);
    assert_eq!(error.kind(), Some(ErrorKind::ProjectNotFound));
    assert!(error.message.contains("register"));
    assert!(
        cli.call(&ProjectsParams {})
            .await
            .unwrap()
            .projects
            .is_empty()
    );
    // Файлы проекта остаются (Т-5).
    assert!(root.join("docs").join("guide.md").exists());
}

#[tokio::test]
async fn similar_chunks_work_by_id_through_the_channel_for_cli_and_mcp() {
    let harness = Harness::start("similar", FakeProvider::new("m1")).await;
    let mut cli = harness.client(ClientKind::Cli).await;
    let root = ready_project(&harness, &mut cli).await;
    let found = cli.call(&search(&root, "индексация")).await.unwrap();
    let first = &found.results[0];
    assert!(!first.chunk_id.is_empty());

    let similar = |chunk_id: &str| SimilarParams {
        target: target(&root),
        chunk_id: chunk_id.into(),
        modules: vec![],
        limit: Some(2),
    };
    let mut mcp = harness.client(ClientKind::Mcp).await;
    for client in [&mut cli, &mut mcp] {
        let out = client.call(&similar(&first.chunk_id)).await.unwrap();
        assert_eq!(out.source.chunk_id, first.chunk_id);
        assert_eq!(out.source.path, first.path);
        assert_eq!(out.limit, 2);
        assert!(!out.results.is_empty() && out.results.len() <= 2);
        assert!(out.results.iter().all(|hit| hit.chunk_id != first.chunk_id));
        assert!(out.results.iter().all(|hit| !hit.chunk_id.is_empty()));
    }

    let narrowed = cli
        .call(&SimilarParams {
            modules: vec!["api".into()],
            ..similar(&first.chunk_id)
        })
        .await
        .unwrap();
    assert!(narrowed.results.iter().all(|hit| hit.module == "api"));

    let unknown = rpc(cli.call(&similar("99999-00000000")).await);
    assert_eq!(unknown.kind(), Some(ErrorKind::ChunkNotFound));
    let malformed = rpc(cli.call(&similar("не-id")).await);
    assert_eq!(malformed.kind(), Some(ErrorKind::InvalidParams));
    let module = rpc(cli
        .call(&SimilarParams {
            modules: vec!["нет-такого".into()],
            ..similar(&first.chunk_id)
        })
        .await);
    assert_eq!(module.kind(), Some(ErrorKind::InvalidParams));
}

#[tokio::test]
async fn duplicates_are_audited_through_the_channel_for_cli_and_mcp() {
    let harness = Harness::start("duplicates", FakeProvider::new("m1")).await;
    let mut cli = harness.client(ClientKind::Cli).await;
    let text = "Таймаут запроса к воркеру равен десяти секундам и не меняется между запусками; \
                значение хранится в настройках проекта и читается при каждом обращении клиента. \
                Изменение значения требует перезапуска воркера, а проверка показывает его рядом с остальными параметрами окна.";
    let root = harness.project(
        "dups",
        &[
            ("a/one.md", &format!("# Один\n\n{text}\n")),
            ("b/two.md", &format!("# Два\n\n{}\n", text.replace("десяти", "тридцати"))),
            ("c/other.md", "# Другое\n\nСовсем другой раздел про нарезку документов на чанки по заголовкам первого и второго уровня в проекте.\n"),
        ],
    );
    cli.call(&RegisterParams {
        path: root.display().to_string(),
    })
    .await
    .unwrap();
    wait_status(&mut cli, &root, "первая индексация", indexed).await;

    let audit = |modules: Vec<String>| DuplicatesParams {
        target: target(&root),
        modules,
        limit: None,
    };
    let mut mcp = harness.client(ClientKind::Mcp).await;
    for client in [&mut cli, &mut mcp] {
        let out = client.call(&audit(vec![])).await.unwrap();
        assert_eq!(out.results.len(), 1, "{out:?}");
        let item = &out.results[0];
        assert_eq!(item.kind, DuplicateKind::Near);
        assert_eq!(item.chunks.len(), 2);
        assert!(item.chunks.iter().all(|chunk| !chunk.chunk_id.is_empty()));
        let differences = item.differences.as_ref().unwrap();
        assert_eq!(differences.changes[0].first, "десяти");
        assert_eq!(differences.changes[0].second, "тридцати");
        assert_eq!(out.counts.near, 1);
        assert_eq!(out.total, 1);
        assert_eq!(out.chunks_checked, 3);
    }

    let narrowed = cli.call(&audit(vec!["a".into()])).await.unwrap();
    assert!(narrowed.results.is_empty());
    let unknown = rpc(cli.call(&audit(vec!["нет-такого".into()])).await);
    assert_eq!(unknown.kind(), Some(ErrorKind::InvalidParams));
}

#[tokio::test]
async fn explicit_project_by_name_and_wrong_calls() {
    let harness = Harness::start("errors", FakeProvider::new("m1")).await;
    let mut cli = harness.client(ClientKind::Cli).await;
    let root = ready_project(&harness, &mut cli).await;
    let elsewhere = harness.projects.path().to_path_buf();
    let by_name = |path: &str| ReadParams {
        target: Target {
            cwd: elsewhere.display().to_string(),
            project: Some("proj".into()),
        },
        path: path.into(),
    };

    assert_eq!(
        cli.call(&by_name("api/rest.md")).await.unwrap().title,
        "rest"
    );
    let missing = rpc(cli.call(&by_name("nope.md")).await);
    assert_eq!(missing.kind(), Some(ErrorKind::FileNotFound));
    let not_md = rpc(cli.call(&by_name("guide.txt")).await);
    assert_eq!(not_md.kind(), Some(ErrorKind::InvalidParams));
    let unknown = rpc(cli
        .call(&ReadParams {
            target: Target {
                cwd: elsewhere.display().to_string(),
                project: Some("нет-такого".into()),
            },
            path: "guide.md".into(),
        })
        .await);
    assert_eq!(unknown.kind(), Some(ErrorKind::ProjectNotFound));

    let bad_level = rpc(cli
        .call(&OutlineParams {
            target: target(&root),
            path: "guide.md".into(),
            levels: vec![7],
            under: None,
            under_line: None,
        })
        .await);
    assert_eq!(bad_level.kind(), Some(ErrorKind::InvalidParams));
    let both = rpc(cli
        .call(&OutlineParams {
            target: target(&root),
            path: "guide.md".into(),
            levels: vec![],
            under: Some("Руководство".into()),
            under_line: Some(1),
        })
        .await);
    assert_eq!(both.kind(), Some(ErrorKind::InvalidParams));
    let empty = rpc(cli.call(&search(&root, "   ")).await);
    assert_eq!(empty.kind(), Some(ErrorKind::InvalidParams));
    let not_dir = rpc(cli
        .call(&RegisterParams {
            path: root.join("docs").join("guide.md").display().to_string(),
        })
        .await);
    assert_eq!(not_dir.kind(), Some(ErrorKind::InvalidParams));
    let relative = rpc(cli.call(&RegisterParams { path: "rel".into() }).await);
    assert_eq!(relative.kind(), Some(ErrorKind::InvalidParams));
}

#[tokio::test]
async fn vanished_project_folder_is_unavailable() {
    let harness = Harness::start("vanished", FakeProvider::new("m1")).await;
    let mut cli = harness.client(ClientKind::Cli).await;
    let root = ready_project(&harness, &mut cli).await;
    let mut mcp = harness.client(ClientKind::Mcp).await;
    assert!(
        mcp.call(&StatusParams {
            target: target(&root)
        })
        .await
        .unwrap()
        .watching
    );
    // Ready относится к тексту; фоновый граф ещё может держать SQLite на Windows.
    wait_status(
        &mut cli,
        &root,
        "граф освободил индекс",
        |s| {
            s.graph
                .as_ref()
                .is_some_and(|g| g.active_job.is_none() && g.snapshot.is_some())
        },
    )
    .await;
    // Idle не запрещает новое короткое чтение; Windows может временно удерживать файл.
    let remove_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match fs::remove_dir_all(&root) {
            Ok(()) => break,
            Err(error)
                if cfg!(windows)
                    && error.raw_os_error() == Some(32)
                    && Instant::now() < remove_deadline =>
            {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(error) => panic!("не удалось подготовить исчезнувший проект: {error}"),
        }
    }
    // Проверку по явной просьбе видит пропажу папки: проект становится недоступным.
    let params = ReindexParams {
        target: target(&root),
        full: false,
        accept_model_change: false,
    };
    cli.call(&params).await.unwrap();
    let status = wait_status(&mut cli, &root, "unavailable", |s| {
        s.state == ProjectState::Unavailable
    })
    .await;
    assert!(!status.watching);
    let search_error = rpc(cli.call(&search(&root, "индексация")).await);
    assert_eq!(search_error.kind(), Some(ErrorKind::ProjectUnavailable));
    let reindex_error = rpc(cli.call(&params).await);
    assert_eq!(reindex_error.kind(), Some(ErrorKind::ProjectUnavailable));
    let projects = cli.call(&ProjectsParams {}).await.unwrap().projects;
    assert_eq!(projects[0].state, ProjectState::Unavailable);
}

#[tokio::test]
async fn mcp_reads_but_cannot_register_or_consent() {
    let provider = FakeProvider::new("m1");
    let harness = Harness::start("mcp", provider.clone()).await;
    let mut cli = harness.client(ClientKind::Cli).await;
    let root = ready_project(&harness, &mut cli).await;
    let mut mcp = harness.client(ClientKind::Mcp).await;

    let error = rpc(mcp
        .call(&RegisterParams {
            path: root.display().to_string(),
        })
        .await);
    assert_eq!(error.code, METHOD_NOT_FOUND);
    let found = mcp.call(&search(&root, "индексация")).await.unwrap();
    assert!(!found.results.is_empty());
    assert!(mcp.call(&ProjectsParams {}).await.is_ok());

    // Смена модели: проверка переводит проект в `reindex_required`.
    provider.set_model("m2");
    let accepted = mcp
        .call(&ReindexParams {
            target: target(&root),
            full: false,
            accept_model_change: false,
        })
        .await
        .unwrap();
    assert!(accepted.accepted);
    wait_status(&mut mcp, &root, "reindex_required", |s| {
        s.state == ProjectState::ReindexRequired
    })
    .await;

    let search_error = rpc(mcp.call(&search(&root, "индексация")).await);
    assert_eq!(search_error.kind(), Some(ErrorKind::ReindexRequired));
    // Агент не может согласиться за человека: флаг от MCP сбрасывается.
    let refused = rpc(mcp
        .call(&ReindexParams {
            target: target(&root),
            full: true,
            accept_model_change: true,
        })
        .await);
    assert_eq!(refused.kind(), Some(ErrorKind::ReindexNeedsConsent));
    assert_eq!(refused.exit_code(), bars_protocol::EXIT_REINDEX);
    let still = mcp
        .call(&StatusParams {
            target: target(&root),
        })
        .await
        .unwrap();
    assert_eq!(still.state, ProjectState::ReindexRequired);
    assert_eq!(still.model.index.as_deref(), Some("builtin:m1"));
    assert_eq!(still.model.settings, "builtin:m2");

    // Человек соглашается через CLI.
    let accepted = cli
        .call(&ReindexParams {
            target: target(&root),
            full: false,
            accept_model_change: true,
        })
        .await
        .unwrap();
    assert!(accepted.accepted);
    wait_status(
        &mut cli,
        &root,
        "переиндексация новой моделью",
        |s| s.state == ProjectState::Ready && s.model.index.as_deref() == Some("builtin:m2"),
    )
    .await;
    let found = mcp.call(&search(&root, "индексация")).await.unwrap();
    assert!(!found.results.is_empty());
}

#[tokio::test]
async fn session_start_registers_a_project_with_docs() {
    let harness = Harness::start("session", FakeProvider::new("m1")).await;
    let mut cli = harness.client(ClientKind::Cli).await;
    let root = harness.project("hooked", &[("a.md", "# A\n\nТекст.\n")]);
    let accepted = cli
        .call(&SessionStartParams {
            cwd: root.display().to_string(),
            source: "startup".into(),
        })
        .await
        .unwrap();
    assert!(accepted.accepted);
    let projects = cli.call(&ProjectsParams {}).await.unwrap().projects;
    assert_eq!(projects.len(), 1);
    wait_status(&mut cli, &root, "индексация по хуку", indexed).await;
}

#[tokio::test]
async fn install_and_uninstall_work_through_the_channel() {
    use bars_protocol::{InstallClient, InstallParams, UninstallParams};
    let harness = Harness::start("install", FakeProvider::new("m1")).await;
    let mut cli = harness.client(ClientKind::Cli).await;
    let out = cli
        .call(&InstallParams {
            client: InstallClient::ClaudeCode,
        })
        .await
        .unwrap();
    assert!(!out.needs_hook_approval);
    assert!(!out.changed_files.is_empty());
    assert!(
        harness
            .home
            .path()
            .join("client-home/.claude.json")
            .exists()
    );
    let out = cli
        .call(&UninstallParams {
            client: InstallClient::ClaudeCode,
        })
        .await
        .unwrap();
    assert!(!out.changed_files.is_empty());
}

#[tokio::test]
async fn second_worker_on_the_same_channel_gives_way() {
    let harness = Harness::start("single", FakeProvider::new("m1")).await;
    let _client = harness.client(ClientKind::Cli).await;
    let home = TempDir::new().unwrap();
    let outcome = serve(
        WorkerConfig::new(home.path().to_path_buf()),
        &harness.endpoint,
        FakeProvider::new("m1"),
    )
    .await;
    assert!(matches!(
        outcome,
        Err(crate::serve::ServeError::AlreadyRunning)
    ));
}

/// Клиент запускает воркер, которого нет: `launch` поднимает его на том же канале.
#[tokio::test]
async fn client_launches_the_worker_on_demand() {
    let endpoint = Endpoint::named("handler-launch");
    let home = TempDir::new().unwrap();
    let config = WorkerConfig::new(home.path().to_path_buf());
    let served = endpoint.clone();
    let mut client = bars_protocol::connect_or_launch_with(
        &endpoint,
        ClientKind::Cli,
        Duration::from_secs(10),
        move || {
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(300)).await;
                let _ = serve(config, &served, FakeProvider::new("m1")).await;
            });
            Ok(())
        },
    )
    .await
    .unwrap();
    assert!(
        client
            .call(&ProjectsParams {})
            .await
            .unwrap()
            .projects
            .is_empty()
    );
}

fn bge_m3_dir() -> Option<PathBuf> {
    if std::env::var_os("HF_HOME").is_some() {
        return None;
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let dir = PathBuf::from(home).join(".bars").join("models");
    let snapshots = dir.join("models--BAAI--bge-m3").join("snapshots");
    fs::read_dir(snapshots)
        .ok()?
        .flatten()
        .any(|snapshot| snapshot.path().join("onnx").join("model.onnx").exists())
        .then_some(dir)
}

#[tokio::test(flavor = "multi_thread")]
async fn search_with_the_real_bge_m3() {
    let Some(models) = bge_m3_dir() else {
        eprintln!("bge-m3 нет в ~/.bars/models: проверка пропущена");
        return;
    };
    let harness = Harness::start("bge", Arc::new(BuiltinProvider::new(models))).await;
    let mut cli = harness.client(ClientKind::Cli).await;
    let root = harness.project(
        "real",
        &[
            (
                "cooking.md",
                "# Борщ\n\nСвёкла, капуста и мясо варятся два часа.\n",
            ),
            (
                "worker.md",
                "# Воркер\n\nОчередь индексации берёт по одному проекту за раз.\n",
            ),
        ],
    );
    cli.call(&RegisterParams {
        path: root.display().to_string(),
    })
    .await
    .unwrap();
    wait_status(&mut cli, &root, "индексация bge-m3", indexed).await;
    let found = cli
        .call(&search(&root, "как устроена очередь индексации"))
        .await
        .unwrap();
    assert_eq!(found.results[0].path, "docs/worker.md");
}

#[tokio::test(flavor = "multi_thread")]
async fn search_answers_model_loading_while_the_model_loads_and_works_after() {
    let provider = Arc::new(FakeProvider {
        background: true,
        load_delay: std::time::Duration::from_millis(600),
        ..FakeProvider::plain("m1")
    });
    let harness = Harness::start("loading", provider).await;
    let mut cli = harness.client(ClientKind::Cli).await;
    let root = harness.project(
        "slow",
        &[(
            "a.md",
            "# Борщ

Свёкла и капуста.
",
        )],
    );
    cli.call(&RegisterParams {
        path: root.display().to_string(),
    })
    .await
    .unwrap();

    // Индексация держит загрузку модели: поиск не ждёт её, а сразу отвечает `model_loading`.
    let started = Instant::now();
    let error = rpc(cli.call(&search(&root, "борщ")).await);
    assert_eq!(error.kind(), Some(ErrorKind::ModelLoading));
    assert!(started.elapsed() < Duration::from_millis(500));
    let data = error.data.expect("data с ходом загрузки");
    assert!(data["percent"].is_u64(), "{data}");

    wait_status(&mut cli, &root, "индексация после загрузки", indexed).await;
    let found = cli.call(&search(&root, "борщ")).await.unwrap();
    assert_eq!(found.results[0].path, "docs/a.md");
}

#[tokio::test(flavor = "multi_thread")]
async fn window_methods_are_closed_on_the_channel() {
    let harness = Harness::start("window-closed", FakeProvider::new("m1")).await;
    let mut cli = harness.client(ClientKind::Cli).await;
    let error = rpc(cli.call(&ActivityParams {}).await);
    assert_eq!(error.code, METHOD_NOT_FOUND);
    let error = rpc(cli.call(&IndexPauseParams {}).await);
    assert_eq!(error.code, METHOD_NOT_FOUND);
}

/// Окно вызывает обработчик напрямую, без канала.
#[tokio::test(flavor = "multi_thread")]
async fn window_gets_activity_tree_and_pause_from_the_handler() {
    let home = TempDir::new().unwrap();
    let projects = TempDir::new().unwrap();
    let root = projects.path().join("alpha");
    fs::create_dir_all(root.join("docs/mod")).unwrap();
    fs::write(root.join("docs/a.md"), "# Альфа\n\nТекст про индексацию.\n").unwrap();
    fs::write(
        root.join("docs/mod/b.md"),
        "# Бета\n\nДругой текст про поиск.\n",
    )
    .unwrap();
    let worker = crate::worker::Worker::start(
        WorkerConfig::new(home.path().to_path_buf()),
        FakeProvider::new("m1"),
    )
    .unwrap();
    let handler = crate::handler::WorkerHandler::new(worker.clone());

    let paused = handler
        .dispatch(Call::IndexPause(IndexPauseParams {}))
        .await
        .unwrap();
    assert_eq!(paused["paused"], true);
    worker.register(&root).unwrap();
    let activity: ActivityOutput = serde_json::from_value(
        handler
            .dispatch(Call::Activity(ActivityParams {}))
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(activity.paused);
    assert!(activity.running.is_none());
    assert_eq!(activity.queue.len(), 1);
    assert_eq!(activity.queue[0].name, "alpha");
    assert_eq!(activity.queue[0].kind, JobKind::Check);
    assert_eq!(activity.queue[0].priority, JobPriority::Explicit);

    let cancelled: CancelOutput = serde_json::from_value(
        handler
            .dispatch(Call::IndexCancel(IndexCancelParams {
                project: root.display().to_string(),
            }))
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(cancelled.cancelled);
    assert!(worker.queued().is_empty());
    worker.register(&root).unwrap();

    handler
        .dispatch(Call::IndexResume(IndexResumeParams {}))
        .await
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !worker.is_idle() {
        assert!(Instant::now() < deadline, "очередь не опустела");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    let tree: TreeOutput = serde_json::from_value(
        handler
            .dispatch(Call::Tree(TreeParams {
                target: target(&root),
            }))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(tree.docs_root, "docs");
    let paths: Vec<&str> = tree
        .documents
        .iter()
        .map(|document| document.path.as_str())
        .collect();
    assert_eq!(paths, ["docs/a.md", "docs/mod/b.md"]);
    assert_eq!(tree.documents[1].module, "mod");
    assert!(tree.documents.iter().all(|document| document.chunks >= 1));

    // После resume слежение вправе поставить новое задание между idle и чтением дерева.
    worker.shutdown().await;
}

mod audits;
mod concurrency;
mod external_models;
mod graph;
mod links;
mod new_topics;
mod window;
