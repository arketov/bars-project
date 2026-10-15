//! Интеграционные тесты `bars-cli`: настоящий воркер (канал, реестр, очередь, индекс) в временных
//! папках, подделка embed-модели, запуск воркера «по требованию» из самого клиента.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bars_cli::{Cli, Env, Launch, run};
use bars_protocol::Endpoint;
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
                let config = WorkerConfig::new(home.clone());
                let (endpoint, provider) = (endpoint.clone(), Arc::clone(&provider));
                handle.spawn(async move { serve(config, &endpoint, provider).await });
                Ok(())
            }))
        };
        let env = Env {
            endpoint,
            cwd: projects.path().to_path_buf(),
            launch,
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

    let status = bars(&env, &["status"]).await;
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
    assert!(parse(&["install", "--client", "codex"]).is_err());
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
