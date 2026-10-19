//! «Новые темы»: сбор из поиска окна и MCP, допуск, удаление. Окно вызывает обработчик напрямую
//! (`dispatch`), агент и CLI идут через настоящий канал; БД читается вторым соединением.

use bars_core::new_topics::{Source, TopicRow, TopicStore};
use bars_core::secrets::MemorySecrets;
use bars_protocol::{ProjectDeleteParams, SearchOutput};

use super::*;
use crate::autostart::MemoryAutostart;
use crate::handler::WorkerHandler;
use crate::new_topics::{Origin, Purpose, SearchContext};
use crate::worker::Worker;

/// Порог выше любого косинуса: поиск успешен, но результатов нет.
const NOTHING: Option<f32> = Some(1.0);

struct Env {
    projects: TempDir,
    worker: Worker,
    handler: WorkerHandler,
    _home: TempDir,
}

impl Env {
    fn new() -> Self {
        let home = TempDir::new().unwrap();
        let mut config = WorkerConfig::new(home.path().to_path_buf());
        config.secrets = Arc::new(MemorySecrets::default());
        config.autostart = Arc::new(MemoryAutostart::new("\"/opt/bars/bars\"", None));
        let worker = Worker::start(config, FakeProvider::new("m1")).unwrap();
        let handler = WorkerHandler::new(worker.clone());
        Self {
            projects: TempDir::new().unwrap(),
            worker,
            handler,
            _home: home,
        }
    }

    async fn ready_project(&self) -> PathBuf {
        let root = self.projects.path().join("proj");
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("docs/a.md"), "# Альфа\n\nТекст про индексацию.\n").unwrap();
        self.worker.register(&root).unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            tokio::time::sleep(Duration::from_millis(60)).await;
            let status = self.worker.status(&root).unwrap();
            if self.worker.is_idle() && status.state == ProjectState::Ready && status.pending == 0 {
                return root;
            }
            assert!(Instant::now() < deadline, "индекс не готов");
        }
    }

    async fn search(
        &self,
        root: &Path,
        query: &str,
        min_score: Option<f32>,
        context: SearchContext,
    ) -> SearchOutput {
        let mut params = search(root, query);
        params.min_score = min_score;
        let value = self
            .handler
            .dispatch_with_context(Call::Search(params), context)
            .await
            .unwrap();
        serde_json::from_value(value).unwrap()
    }

    /// Темы после того, как писатель отработал очередь: ждём, пока их число совпадёт с ожидаемым.
    async fn topics(&self, root: &Path, observations: i64) -> Vec<TopicRow> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let topics = self.handler.topics().list(root).await.unwrap();
            if topics.iter().map(|t| t.observations).sum::<i64>() == observations {
                return topics;
            }
            assert!(
                Instant::now() < deadline,
                "ждали {observations} наблюдений: {topics:?}"
            );
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }

    /// Пауза, за которую лишнее наблюдение успело бы записаться.
    async fn settle() {
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
}

fn window() -> SearchContext {
    SearchContext::default()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn empty_window_search_becomes_a_topic_and_repeats_merge() {
    let env = Env::new();
    let root = env.ready_project().await;
    let out = env.search(&root, "как включить сбор?", NOTHING, window()).await;
    assert!(out.results.is_empty());
    let topics = env.topics(&root, 1).await;
    assert_eq!(topics.len(), 1);
    assert_eq!(topics[0].variants[0].scope, "project");

    // Тот же вопрос с другим регистром и пробелами — та же тема.
    env.search(&root, "Как  включить сбор?", NOTHING, window()).await;
    let topics = env.topics(&root, 2).await;
    assert_eq!(topics.len(), 1);
    assert_eq!(topics[0].observations, 2);
    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn diagnostics_and_non_empty_results_do_not_create_demand() {
    let env = Env::new();
    let root = env.ready_project().await;
    let diagnostic = SearchContext {
        purpose: Purpose::Diagnostic,
        ..Default::default()
    };
    env.search(&root, "диагностика пустой выдачи", NOTHING, diagnostic)
        .await;
    // Непустая выдача автоматической темы не создаёт.
    let found = env
        .search(&root, "индексацию", Some(0.0), window())
        .await;
    assert!(!found.results.is_empty());
    Env::settle().await;
    assert!(env.handler.topics().list(&root).await.unwrap().is_empty());
    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scope_is_kept_and_a_repeated_request_id_is_counted_once() {
    let env = Env::new();
    let root = env.ready_project().await;
    let mut params = search(&root, "вопрос в области");
    params.min_score = NOTHING;
    params.modules = vec![".".into()];
    let context = SearchContext {
        request_id: Some("window:test:1".into()),
        ..Default::default()
    };
    for _ in 0..2 {
        env.handler
            .dispatch_with_context(Call::Search(params.clone()), context.clone())
            .await
            .unwrap();
    }
    let topics = env.topics(&root, 1).await;
    assert_eq!(topics[0].variants[0].scope, "m=.;f=");
    Env::settle().await;
    assert_eq!(env.topics(&root, 1).await.len(), 1, "повторный ID не учтён");
    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disabling_and_clearing_stop_late_and_new_records() {
    let env = Env::new();
    let root = env.ready_project().await;
    env.search(&root, "первый вопрос", NOTHING, window()).await;
    env.topics(&root, 1).await;

    env.handler.topics().set_enabled(&root, false).await.unwrap();
    env.search(&root, "второй вопрос", NOTHING, window()).await;
    Env::settle().await;
    assert_eq!(env.topics(&root, 1).await.len(), 1, "выключенный сбор не пишет");

    env.handler.topics().set_enabled(&root, true).await.unwrap();
    env.search(&root, "третий вопрос", NOTHING, window()).await;
    env.topics(&root, 2).await;

    env.handler.topics().clear(&root).await.unwrap();
    assert!(env.handler.topics().list(&root).await.unwrap().is_empty());
    // Очистка не отключает сбор: следующее обращение создаёт тему заново.
    env.search(&root, "первый вопрос", NOTHING, window()).await;
    assert_eq!(env.topics(&root, 1).await.len(), 1);
    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn not_ready_index_does_not_create_a_gap() {
    let env = Env::new();
    let root = env.ready_project().await;
    // Индекс в очереди: пустая выдача может быть следствием неполного индекса.
    env.worker.pause();
    fs::write(root.join("docs/b.md"), "# Бета\n\nНовый документ.\n").unwrap();
    env.worker.reindex(&root, false, false).unwrap();
    let status = env.worker.status(&root).unwrap();
    assert!(status.queued || status.pending > 0 || status.state == ProjectState::Indexing);
    env.search(&root, "вопрос при индексации", NOTHING, window()).await;
    Env::settle().await;
    assert!(env.handler.topics().list(&root).await.unwrap().is_empty());
    env.worker.resume();
    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn project_deletion_removes_the_topics_database_and_blocks_late_writes() {
    let env = Env::new();
    let root = env.ready_project().await;
    env.search(&root, "вопрос до удаления", NOTHING, window()).await;
    env.topics(&root, 1).await;
    assert!(bars_core::new_topics::store_path(&root).is_file());

    let out = env
        .handler
        .dispatch(Call::ProjectDelete(ProjectDeleteParams {
            project: root.display().to_string(),
            confirmed: true,
        }))
        .await
        .unwrap();
    assert_eq!(out["removed"], true);
    assert!(!root.join(".bars").exists(), "БД и sidecar удалены вместе с .bars");
    // Поздняя запись не воскрешает папку проекта.
    assert!(env.handler.topics().list(&root).await.is_err());
    assert!(!root.join(".bars").exists());
    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_collects_and_cli_does_not_through_the_channel() {
    let harness = Harness::start("new-topics", FakeProvider::new("m1")).await;
    let mut cli = harness.client(ClientKind::Cli).await;
    let root = ready_project(&harness, &mut cli).await;
    let mut empty = search(&root, "вопрос агента без ответа");
    empty.min_score = NOTHING;

    cli.call(&empty).await.unwrap();
    let mut mcp = harness.client(ClientKind::Mcp).await;
    mcp.call(&empty).await.unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    let topics = loop {
        let topics = TopicStore::open(&root).unwrap().list().unwrap();
        if !topics.is_empty() || Instant::now() > deadline {
            break topics;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    };
    assert_eq!(topics.len(), 1);
    assert_eq!(topics[0].observations, 1, "CLI спроса не создаёт");
    let db = rusqlite::Connection::open(bars_core::new_topics::store_path(&root)).unwrap();
    let source: String = db
        .query_row("SELECT source FROM observations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(source, Source::Mcp.as_str());
    // Служебные методы окна недоступны агенту, поле `_window` не часть протокола.
    assert_eq!(Origin::default(), Origin::Window);
}

#[test]
fn window_context_is_taken_out_of_the_params() {
    let mut params = serde_json::json!({
        "cwd": "x", "query": "q",
        "_window": {"purpose": "diagnostic", "request_id": "r1", "parent_request_id": "r0"}
    });
    let context = SearchContext::take_from(&mut params);
    assert_eq!(context.purpose, Purpose::Diagnostic);
    assert_eq!(context.request_id.as_deref(), Some("r1"));
    assert_eq!(context.parent_request_id.as_deref(), Some("r0"));
    assert!(params.get("_window").is_none());
    let mut plain = serde_json::json!({"cwd": "x"});
    assert_eq!(SearchContext::take_from(&mut plain).purpose, Purpose::Request);
}
