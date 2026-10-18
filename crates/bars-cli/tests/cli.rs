//! Интеграционные тесты `bars-cli`: настоящий воркер (канал, реестр, очередь, индекс) в временных
//! папках, подделка embed-модели, запуск воркера «по требованию» из самого клиента.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bars_cli::{Cli, Env, Launch, run};
use bars_protocol::Endpoint;
use bars_worker::clients::InstallEnv;
use bars_worker::serve::serve;
use bars_worker::worker::WorkerConfig;
use bars_worker::worker::models::fake::FakeProvider;
use clap::Parser;
use serde_json::Value;
use tempfile::TempDir;

struct Harness {
    home: TempDir,
    projects: TempDir,
    env: Env,
    provider: Arc<FakeProvider>,
}

struct Outcome {
    code: i32,
    out: String,
    err: String,
}

impl Harness {
    /// Воркер ещё не запущен: его запустит первая же команда через `Launch`.
    fn new(name: &str) -> Self {
        let home = TempDir::new().unwrap();
        let projects = TempDir::new().unwrap();
        let provider = FakeProvider::new("m1");
        let endpoint = Endpoint::named(&format!("cli-{name}"));
        let handle = tokio::runtime::Handle::current();
        let launch = {
            let (home, endpoint, provider) = (
                home.path().to_path_buf(),
                endpoint.clone(),
                Arc::clone(&provider),
            );
            Launch::With(Arc::new(move || {
                let mut config = WorkerConfig::new(home.clone());
                // Настоящие ~/.claude и ~/.codex не трогаются: клиенты живут во временной папке.
                config.install_env = Some(InstallEnv {
                    home: home.join("client-home"),
                    mcp_exe: PathBuf::from("/opt/bars/bars-mcp"),
                    cli_exe: PathBuf::from("/opt/bars/bars-cli"),
                });
                let (endpoint, provider) = (endpoint.clone(), Arc::clone(&provider));
                handle.spawn(async move { serve(config, &endpoint, provider).await });
                Ok(())
            }))
        };
        let env = Env {
            endpoint,
            cwd: projects.path().to_path_buf(),
            launch,
            hook_input: None,
        };
        Self {
            home,
            projects,
            env,
            provider,
        }
    }

    fn env_in(&self, cwd: &Path) -> Env {
        Env {
            cwd: cwd.to_path_buf(),
            ..self.env.clone()
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
        fs::write(root.join("notes.txt"), "не документ").unwrap();
        root
    }
}

const GUIDE: &str = "---\ntitle: Руководство\n---\n# Руководство\n\nВведение.\n\n## Индексация\n\n\
    Индексация документов идёт в очереди воркера.\n\n### Слежение\n\nСлежение за файлами.\n\n\
    ## Поиск\n\nПоиск работает по индексу.\n";

async fn bars(env: &Env, args: &[&str]) -> Outcome {
    let cli = Cli::try_parse_from(std::iter::once("bars-cli").chain(args.iter().copied()))
        .unwrap_or_else(|error| panic!("аргументы {args:?} не разобрались: {error}"));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run(cli, env, &mut out, &mut err).await;
    Outcome {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

fn json(outcome: &Outcome) -> Value {
    assert_eq!(outcome.code, 0, "stderr: {}", outcome.err);
    serde_json::from_str(&outcome.out).expect("вывод --json — один JSON")
}

/// Ждёт, пока `status --json` не покажет нужное.
async fn wait_status(env: &Env, what: &str, condition: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let outcome = bars(env, &["--json", "status"]).await;
        if outcome.code == 0 {
            let status = json(&outcome);
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
async fn register_search_outline_read_scenario() {
    let harness = Harness::new("scenario");
    let root = harness.project("proj");
    let env = harness.env_in(&root);

    // Первая команда запускает воркер сама.
    let registered = bars(&env, &["register"]).await;
    assert_eq!(registered.code, 0, "stderr: {}", registered.err);
    assert!(registered.out.contains("Проект зарегистрирован"));
    assert!(registered.out.contains("корень документации: docs"));
    wait_status(&env, "первая индексация", indexed).await;

    // Из подпапки проект находится вверх по дереву (Т-15).
    let sub = harness.env_in(&root.join("docs").join("api"));
    let found = bars(&sub, &["search", "индексация", "документов"]).await;
    assert_eq!(found.code, 0, "stderr: {}", found.err);
    assert!(found.out.contains("1. docs/guide.md:"), "{}", found.out);
    assert!(found.out.contains("Руководство — "), "{}", found.out);
    assert!(
        found.out.contains("Применено: результатов"),
        "{}",
        found.out
    );

    let hits = json(&bars(&env, &["--json", "search", "индексация", "--limit", "1"]).await);
    assert_eq!(hits["results"].as_array().unwrap().len(), 1);
    assert_eq!(hits["results"][0]["path"], "docs/guide.md");
    assert_eq!(hits["applied"]["results"], 1);

    let scoped = json(&bars(&env, &["--json", "search", "REST", "--module", "api"]).await);
    assert!(
        scoped["results"]
            .as_array()
            .unwrap()
            .iter()
            .all(|hit| hit["module"] == "api")
    );

    // Строки структуры — `номер ## текст`, номера совпадают с диском.
    let outline = bars(&env, &["outline", "docs/guide.md", "--levels", "2,3"]).await;
    assert_eq!(outline.code, 0, "stderr: {}", outline.err);
    assert_eq!(
        outline.out,
        "8 ## Индексация\n12 ### Слежение\n16 ## Поиск\n"
    );
    let under = bars(
        &env,
        &["outline", "guide.md", "--under", "Руководство/Индексация"],
    )
    .await;
    assert_eq!(under.out, "12 ### Слежение\n");
    let under_line = bars(&env, &["outline", "guide.md", "--under-line", "8"]).await;
    assert_eq!(under_line.out, "12 ### Слежение\n");
    let outline_json = json(&bars(&env, &["--json", "outline", "guide.md", "--levels", "1"]).await);
    assert_eq!(outline_json["headings"][0]["text"], "Руководство");

    // Документ целиком с теми же номерами строк.
    let read = bars(&env, &["read", "guide.md"]).await;
    assert_eq!(read.code, 0, "stderr: {}", read.err);
    assert!(
        read.out.starts_with("Руководство\n\n1 ---\n"),
        "{}",
        read.out
    );
    assert!(read.out.contains("\n8 ## Индексация\n"), "{}", read.out);
    let read_json = json(&bars(&env, &["--json", "read", "guide.md"]).await);
    assert_eq!(read_json["title"], "Руководство");

    let projects = bars(&env, &["projects"]).await;
    assert!(projects.out.contains("(proj)"), "{}", projects.out);
    assert!(projects.out.contains("api (1)"), "{}", projects.out);
    let projects_json = json(&bars(&env, &["--json", "projects"]).await);
    assert_eq!(projects_json["projects"][0]["name"], "proj");

    // A watcher follow-up may overlap the status request after the initial indexing.
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        let status = bars(&env, &["status"]).await;
        if status.out.contains("Состояние: готов") || Instant::now() >= deadline {
            break status;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    };
    assert!(status.out.contains("Состояние: готов"), "{}", status.out);

    // Свой проект можно задать явно и из другой папки.
    let elsewhere = harness.env_in(harness.home.path());
    let project_flag = root.display().to_string();
    let explicit = bars(
        &elsewhere,
        &["--project", &project_flag, "read", "guide.md"],
    )
    .await;
    assert_eq!(explicit.code, 0, "stderr: {}", explicit.err);

    let reindex = bars(&env, &["reindex", "--full"]).await;
    assert_eq!(reindex.code, 0, "stderr: {}", reindex.err);
    assert!(reindex.out.contains("Переиндексация принята"));
    wait_status(&env, "полная переиндексация", indexed).await;

    let removed = bars(&env, &["unregister"]).await;
    assert_eq!(removed.code, 0, "stderr: {}", removed.err);
    assert!(removed.out.contains("снят с регистрации"));
    assert!(root.join("docs/guide.md").exists());
    let gone = bars(&env, &["status"]).await;
    assert_eq!(gone.code, 1);
    assert!(gone.err.contains("bars-cli register"), "{}", gone.err);
}

#[tokio::test(flavor = "multi_thread")]
async fn request_errors_exit_with_one() {
    let harness = Harness::new("errors");
    let root = harness.project("proj");
    let env = harness.env_in(&root);
    assert_eq!(bars(&env, &["register"]).await.code, 0);
    wait_status(&env, "первая индексация", indexed).await;

    let missing = bars(&env, &["read", "no-such.md"]).await;
    assert_eq!(missing.code, 1);
    assert!(missing.err.starts_with("bars-cli: "), "{}", missing.err);
    let not_markdown = bars(&env, &["read", "notes.txt"]).await;
    assert_eq!(not_markdown.code, 1);
    let empty_query = bars(&env, &["search", "  "]).await;
    assert_eq!(empty_query.code, 1);
    let bad_level = bars(&env, &["outline", "guide.md", "--under", "Нет такого"]).await;
    assert_eq!(bad_level.code, 1);

    // С --json ошибка — объект с видом ошибки.
    let failed = bars(&env, &["--json", "read", "no-such.md"]).await;
    assert_eq!(failed.code, 1);
    let error: Value = serde_json::from_str(&failed.err).unwrap();
    assert_eq!(error["error"]["kind"], "file_not_found");
    assert_eq!(error["error"]["code"], -32006);
    assert!(failed.out.is_empty());

    // Вне зарегистрированных проектов.
    let outside = harness.env_in(harness.home.path());
    let lost = bars(&outside, &["search", "что-нибудь"]).await;
    assert_eq!(lost.code, 1);
    assert!(lost.err.contains("register"), "{}", lost.err);
}

#[tokio::test(flavor = "multi_thread")]
async fn unreachable_worker_exits_with_two() {
    let home = TempDir::new().unwrap();
    let env = Env {
        endpoint: Endpoint::named("cli-no-worker"),
        cwd: home.path().to_path_buf(),
        hook_input: None,
        launch: Launch::With(Arc::new(|| {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "нет bars",
            ))
        })),
    };
    let outcome = bars(&env, &["projects"]).await;
    assert_eq!(outcome.code, 2);
    assert!(outcome.err.contains("нет bars"), "{}", outcome.err);
    assert!(outcome.out.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn model_change_needs_consent_and_exits_with_three() {
    let harness = Harness::new("consent");
    let root = harness.project("proj");
    let env = harness.env_in(&root);
    assert_eq!(bars(&env, &["register"]).await.code, 0);
    wait_status(&env, "первая индексация", indexed).await;

    // Смена модели замечается при проверке.
    harness.provider.set_model("m2");
    assert_eq!(bars(&env, &["reindex"]).await.code, 0);
    wait_status(&env, "reindex_required", |s| {
        s["state"] == "reindex_required"
    })
    .await;

    let status = bars(&env, &["status"]).await;
    assert!(
        status.out.contains("нужна переиндексация"),
        "{}",
        status.out
    );
    assert!(
        status.out.contains("--accept-model-change"),
        "{}",
        status.out
    );

    let search = bars(&env, &["search", "индексация"]).await;
    assert_eq!(search.code, 3, "stderr: {}", search.err);
    assert!(
        search.err.contains("--accept-model-change"),
        "{}",
        search.err
    );
    let no_consent = bars(&env, &["reindex", "--full"]).await;
    assert_eq!(no_consent.code, 3);
    let kind = bars(&env, &["--json", "reindex"]).await;
    assert_eq!(kind.code, 3);
    let error: Value = serde_json::from_str(&kind.err).unwrap();
    assert_eq!(error["error"]["kind"], "reindex_needs_consent");

    // Согласие человека запускает переиндексацию, поиск снова работает.
    let accepted = bars(&env, &["reindex", "--accept-model-change"]).await;
    assert_eq!(accepted.code, 0, "stderr: {}", accepted.err);
    wait_status(&env, "переиндексация на новую модель", indexed).await;
    assert_eq!(bars(&env, &["search", "индексация"]).await.code, 0);
}

#[test]
fn arguments_are_checked_before_the_worker_is_called() {
    let parse = |args: &[&str]| {
        Cli::try_parse_from(std::iter::once("bars-cli").chain(args.iter().copied()))
    };
    assert!(parse(&["search"]).is_err());
    assert!(parse(&["outline", "a.md", "--levels", "0"]).is_err());
    assert!(parse(&["outline", "a.md", "--levels", "7"]).is_err());
    assert!(parse(&["outline", "a.md", "--under", "A", "--under-line", "3"]).is_err());
    assert!(parse(&["install"]).is_err());
    assert!(parse(&["install", "--client", "other"]).is_err());
    assert!(parse(&["install", "--client", "codex", "--print-instruction"]).is_err());
    let cli = parse(&[
        "search",
        "несколько",
        "слов",
        "--json",
        "--module",
        "a",
        "--module",
        "b",
    ])
    .unwrap();
    assert!(cli.json);
}

#[tokio::test(flavor = "multi_thread")]
async fn install_and_uninstall_edit_client_files_in_temp_home() {
    let harness = Harness::new("install");
    let env = harness.env.clone();
    let client_home = harness.home.path().join("client-home");

    let installed = bars(&env, &["--json", "install", "--client", "codex"]).await;
    let value = json(&installed);
    assert_eq!(value["needs_hook_approval"], true);
    assert!(
        value["changed_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file.as_str().unwrap().ends_with("config.toml"))
    );
    let config = fs::read_to_string(client_home.join(".codex/config.toml")).unwrap();
    assert!(config.contains("[mcp_servers.bars]"), "{config}");

    let human = bars(&env, &["install", "--client", "codex"]).await;
    assert_eq!(human.code, 0, "stderr: {}", human.err);
    assert!(human.out.contains("уже установлен"), "{}", human.out);
    assert!(!human.out.contains("/hooks"), "{}", human.out);

    let hooks_file = client_home.join(".codex/hooks.json");
    let hooks = fs::read_to_string(&hooks_file)
        .unwrap()
        .replace("startup|resume", "startup");
    fs::write(&hooks_file, hooks).unwrap();
    let changed_hook = bars(&env, &["install", "--client", "codex"]).await;
    assert_eq!(changed_hook.code, 0, "{}", changed_hook.err);
    assert!(changed_hook.out.contains("/hooks"), "{}", changed_hook.out);

    let removed = json(&bars(&env, &["--json", "uninstall", "--client", "codex"]).await);
    assert!(!removed["changed_files"].as_array().unwrap().is_empty());
    let config = fs::read_to_string(client_home.join(".codex/config.toml")).unwrap();
    assert!(!config.contains("bars"), "{config}");
}

#[tokio::test(flavor = "multi_thread")]
async fn print_instruction_needs_no_worker() {
    let harness = Harness::new("instruction");
    let out = bars(&harness.env, &["install", "--print-instruction"]).await;
    assert_eq!(out.code, 0, "stderr: {}", out.err);
    assert!(
        out.out.contains("install --client claude-code"),
        "{}",
        out.out
    );
    assert!(out.out.contains("--accept-model-change"), "{}", out.out);
}

#[tokio::test(flavor = "multi_thread")]
async fn session_start_hook_registers_project_and_never_fails() {
    let harness = Harness::new("hook");
    let root = harness.project("hooked");
    // Корень по git: сессия открыта в подпапке документации.
    fs::create_dir(root.join(".git")).unwrap();
    let mut env = harness.env_in(&root);
    env.hook_input = Some(format!(
        "{{\"cwd\": {}, \"source\": \"startup\"}}",
        serde_json::to_string(&root.join("docs").display().to_string()).unwrap()
    ));
    let hook = bars(&env, &["hook", "session-start"]).await;
    assert_eq!(hook.code, 0, "stderr: {}", hook.err);
    assert!(hook.out.is_empty());
    wait_status(&harness.env_in(&root), "индексация после хука", indexed).await;
    let projects = json(&bars(&env, &["--json", "projects"]).await);
    assert_eq!(projects["projects"].as_array().unwrap().len(), 1);
    // BARS_INSTRUCTION.md появляется после проверки актуальности (Т-8).
    assert!(root.join("docs/BARS_INSTRUCTION.md").exists());

    // Мусор на входе: код выхода всё равно 0.
    env.hook_input = Some("не json".into());
    let bad = bars(&env, &["hook", "session-start"]).await;
    assert_eq!(bad.code, 0);
    assert!(bad.err.contains("bars-cli hook"), "{}", bad.err);

    // Воркер недоступен — тоже 0.
    let dead = Env {
        launch: Launch::With(Arc::new(|| Ok(()))),
        endpoint: Endpoint::named("cli-hook-dead"),
        ..env
    };
    assert_eq!(bars(&dead, &["hook", "session-start"]).await.code, 0);
}
