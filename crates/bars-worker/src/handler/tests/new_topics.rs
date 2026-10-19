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
    let out = env
        .search(&root, "как включить сбор?", NOTHING, window())
        .await;
    assert!(out.results.is_empty());
    let topics = env.topics(&root, 1).await;
    assert_eq!(topics.len(), 1);
    assert_eq!(topics[0].variants[0].scope, "project");

    // Тот же вопрос с другим регистром и пробелами — та же тема.
    env.search(&root, "Как  включить сбор?", NOTHING, window())
        .await;
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
    let found = env.search(&root, "индексацию", Some(0.0), window()).await;
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

    env.handler
        .topics()
        .set_enabled(&root, false)
        .await
        .unwrap();
    env.search(&root, "второй вопрос", NOTHING, window()).await;
    Env::settle().await;
    assert_eq!(
        env.topics(&root, 1).await.len(),
        1,
        "выключенный сбор не пишет"
    );

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
    env.search(&root, "вопрос при индексации", NOTHING, window())
        .await;
    Env::settle().await;
    assert!(env.handler.topics().list(&root).await.unwrap().is_empty());
    env.worker.resume();
    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn project_deletion_removes_the_topics_database_and_blocks_late_writes() {
    let env = Env::new();
    let root = env.ready_project().await;
    env.search(&root, "вопрос до удаления", NOTHING, window())
        .await;
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
    assert!(
        !root.join(".bars").exists(),
        "БД и sidecar удалены вместе с .bars"
    );
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
    assert_eq!(
        SearchContext::take_from(&mut plain).purpose,
        Purpose::Request
    );
}

// --- Внутренний API окна (P3) ---

use bars_protocol::{
    NewTopicDetail, NewTopicsCheckParams, NewTopicsClearParams, NewTopicsDeleteParams,
    NewTopicsGetParams, NewTopicsListOutput, NewTopicsListParams, NewTopicsMarkMissingOutput,
    NewTopicsMarkMissingParams, NewTopicsRenameParams, NewTopicsSetStatusParams,
    NewTopicsSettingsParams,
};

impl Env {
    /// Проект без единого документа: поиск успешен и всегда пуст, индекс готов.
    async fn empty_corpus(&self) -> PathBuf {
        let root = self.projects.path().join("empty");
        fs::create_dir_all(root.join("docs")).unwrap();
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

    async fn window_call<T: serde::de::DeserializeOwned>(&self, call: Call) -> T {
        serde_json::from_value(self.handler.dispatch(call).await.unwrap()).unwrap()
    }

    async fn window_error(&self, call: Call) -> bars_protocol::RpcError {
        self.handler.dispatch(call).await.unwrap_err()
    }

    async fn list(&self, root: &Path) -> NewTopicsListOutput {
        self.window_call(Call::NewTopicsList(NewTopicsListParams {
            project: root.display().to_string(),
        }))
        .await
    }

    async fn detail(&self, root: &Path, id: i64) -> NewTopicDetail {
        self.window_call(Call::NewTopicsGet(NewTopicsGetParams {
            project: root.display().to_string(),
            id,
        }))
        .await
    }

    async fn mark(
        &self,
        root: &Path,
        request_id: &str,
        query: &str,
        modules: &[&str],
    ) -> NewTopicsMarkMissingOutput {
        self.window_call(Call::NewTopicsMarkMissing(NewTopicsMarkMissingParams {
            project: root.display().to_string(),
            request_id: request_id.into(),
            query: query.into(),
            modules: modules.iter().map(|m| m.to_string()).collect(),
            files: vec![],
        }))
        .await
    }

    fn check_call(root: &Path, topic: &NewTopicDetail, expand: bool) -> Call {
        Call::NewTopicsCheck(NewTopicsCheckParams {
            project: root.display().to_string(),
            id: topic.topic.id,
            revision: topic.topic.revision,
            expand,
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn manual_mark_creates_a_topic_with_a_non_empty_result_and_is_idempotent() {
    let env = Env::new();
    let root = env.ready_project().await;
    // Выдача непустая: автоматической темы нет, пользователь отмечает вручную.
    let shown = env.search(&root, "индексацию", Some(0.0), window()).await;
    assert!(!shown.results.is_empty());
    Env::settle().await;
    assert!(env.list(&root).await.topics.is_empty());

    let first = env.mark(&root, "window:r1", "индексацию", &[]).await;
    assert!(first.created);
    let again = env.mark(&root, "window:r1", "индексацию", &[]).await;
    assert_eq!((again.created, again.topic_id), (false, first.topic_id));
    let listed = env.list(&root).await;
    assert_eq!(listed.topics.len(), 1);
    assert_eq!(
        listed.topics[0].observations, 1,
        "повтор той же отметки не считается"
    );
    assert_eq!(listed.topics[0].reasons, vec!["manual_missing"]);
    assert_eq!(listed.topics[0].sources, vec!["window"]);
    assert!(listed.enabled && listed.store_path.ends_with("new-topics.sqlite3"));

    // Выключенный сбор не принимает и ручные отметки.
    env.window_call::<bars_protocol::NewTopicsSettingsOutput>(Call::NewTopicsSettings(
        NewTopicsSettingsParams {
            project: root.display().to_string(),
            enabled: false,
        },
    ))
    .await;
    let error = env
        .window_error(Call::NewTopicsMarkMissing(NewTopicsMarkMissingParams {
            project: root.display().to_string(),
            request_id: "window:r2".into(),
            query: "другой вопрос".into(),
            modules: vec![],
            files: vec![],
        }))
        .await;
    assert_eq!(error.kind(), Some(ErrorKind::InvalidParams));
    assert_eq!(env.list(&root).await.topics.len(), 1);
    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn check_finds_materials_without_adding_demand_and_keeps_the_user_decision() {
    let env = Env::new();
    let root = env.ready_project().await;
    env.search(&root, "как устроена индексация", NOTHING, window())
        .await;
    let topic = env.topics(&root, 1).await.remove(0);

    let before = env.detail(&root, topic.id).await;
    assert!(before.checks.is_empty());
    let checked: NewTopicDetail = env
        .window_call(Env::check_call(&root, &before, false))
        .await;
    assert_eq!(checked.checks.len(), 1);
    let check = &checked.checks[0];
    assert_eq!(check.results.len(), 1);
    assert_eq!(check.results[0].outcome, "found", "{check:?}");
    assert!(!check.results[0].hits.is_empty());
    assert!(check.conclusive && !check.expanded);
    assert_eq!(checked.topic.status, "materials_found");
    // Переход в «Есть материалы» сделала сама проверка: она относится к новой ревизии и не устаревшая.
    assert_eq!(check.topic_revision, checked.topic.revision);
    assert_eq!(checked.topic.revision, before.topic.revision + 1);

    // Проверка не спрос: наблюдений по-прежнему одно.
    Env::settle().await;
    assert_eq!(env.detail(&root, topic.id).await.topic.observations, 1);

    // Ревизия устарела (тема изменилась проверкой): повторная проверка по ней — конфликт.
    let error = env
        .window_error(Env::check_call(&root, &before, false))
        .await;
    assert_eq!(error.kind(), Some(ErrorKind::StaleRef));

    // Решение пользователя; материалы последней проверки не стираются.
    let revision = checked.topic.revision;
    let resolved: bars_protocol::NewTopicsChangedOutput = env
        .window_call(Call::NewTopicsSetStatus(NewTopicsSetStatusParams {
            project: root.display().to_string(),
            id: topic.id,
            revision,
            status: "resolved".into(),
        }))
        .await;
    let after = env.detail(&root, topic.id).await;
    assert_eq!(after.topic.status, "resolved");
    assert_eq!(after.topic.revision, resolved.revision);
    assert_eq!(after.checks.len(), 1);
    let error = env
        .window_error(Call::NewTopicsSetStatus(NewTopicsSetStatusParams {
            project: root.display().to_string(),
            id: topic.id,
            revision,
            status: "open".into(),
        }))
        .await;
    assert_eq!(error.kind(), Some(ErrorKind::StaleRef), "старая ревизия");
    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn empty_check_leaves_the_topic_open_and_a_missing_scope_is_not_widened_silently() {
    let env = Env::new();
    let root = env.empty_corpus().await;
    env.search(&root, "вопрос в пустом корпусе", NOTHING, window())
        .await;
    let topic = env.topics(&root, 1).await.remove(0);
    let before = env.detail(&root, topic.id).await;
    let checked: NewTopicDetail = env
        .window_call(Env::check_call(&root, &before, false))
        .await;
    assert_eq!(checked.checks[0].results[0].outcome, "empty");
    assert!(checked.checks[0].conclusive);
    assert_eq!(checked.topic.status, "open");

    // Область, которой больше нет: неполный итог, а не «пусто» и не поиск по всему проекту.
    let marked = env
        .mark(
            &root,
            "window:gone",
            "вопрос из исчезнувшей области",
            &["нет-такого"],
        )
        .await;
    let scoped = env.detail(&root, marked.topic_id).await;
    let incomplete: NewTopicDetail = env
        .window_call(Env::check_call(&root, &scoped, false))
        .await;
    let result = &incomplete.checks[0].results[0];
    assert_eq!(result.outcome, "incomplete", "{result:?}");
    assert_eq!(result.reason.as_deref(), Some("scope_missing"));
    assert_eq!(result.scope, "m=нет-такого;f=");
    assert!(!incomplete.checks[0].conclusive);
    assert_eq!(incomplete.topic.status, "open");

    // Явное расширение подписано и ищет по всему проекту.
    let again = env.detail(&root, marked.topic_id).await;
    let widened: NewTopicDetail = env.window_call(Env::check_call(&root, &again, true)).await;
    assert!(widened.checks[0].expanded);
    assert_eq!(widened.checks[0].results[0].scope, "project");
    assert_eq!(widened.checks[0].results[0].outcome, "empty");
    // Прежняя проверка осталась как предыдущая.
    assert_eq!(widened.checks.len(), 2);
    assert_eq!(widened.checks[1].results[0].outcome, "incomplete");
    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn delete_clear_and_rename_do_not_resurrect_data() {
    let env = Env::new();
    let root = env.empty_corpus().await;
    let a = env.mark(&root, "window:a", "первая тема", &[]).await;
    let b = env
        .mark(&root, "window:b", "вторая тема совсем другая", &[])
        .await;
    let topic = env.detail(&root, a.topic_id).await;

    // Название меняется, формулировка остаётся.
    let renamed: bars_protocol::NewTopicsChangedOutput = env
        .window_call(Call::NewTopicsRename(NewTopicsRenameParams {
            project: root.display().to_string(),
            id: a.topic_id,
            revision: topic.topic.revision,
            title: "  Моё название ".into(),
        }))
        .await;
    let titled = env.detail(&root, a.topic_id).await;
    assert_eq!(titled.topic.title, "Моё название");
    assert_eq!(titled.variants_list[0].text, "первая тема");
    assert_eq!(titled.topic.revision, renamed.revision);

    // Удаление по устаревшей ревизии отклонено, по актуальной — удаляет.
    let stale = env
        .window_error(Call::NewTopicsDelete(NewTopicsDeleteParams {
            project: root.display().to_string(),
            id: a.topic_id,
            revision: topic.topic.revision,
        }))
        .await;
    assert_eq!(stale.kind(), Some(ErrorKind::StaleRef));
    env.window_call::<bars_protocol::AcceptedOutput>(Call::NewTopicsDelete(
        NewTopicsDeleteParams {
            project: root.display().to_string(),
            id: a.topic_id,
            revision: renamed.revision,
        },
    ))
    .await;
    // Проверка удалённой темы не воскрешает её.
    let error = env
        .window_error(Env::check_call(&root, &titled, false))
        .await;
    assert_eq!(error.kind(), Some(ErrorKind::StaleRef));
    assert_eq!(env.list(&root).await.topics.len(), 1);

    // Очистка требует подтверждения; сбор остаётся включённым, поколение растёт.
    let unconfirmed = env
        .window_error(Call::NewTopicsClear(NewTopicsClearParams {
            project: root.display().to_string(),
            confirmed: false,
        }))
        .await;
    assert_eq!(unconfirmed.kind(), Some(ErrorKind::InvalidParams));
    let generation = env.list(&root).await.generation;
    let cleared: bars_protocol::NewTopicsGenerationOutput = env
        .window_call(Call::NewTopicsClear(NewTopicsClearParams {
            project: root.display().to_string(),
            confirmed: true,
        }))
        .await;
    assert!(cleared.generation > generation);
    let listed = env.list(&root).await;
    assert!(listed.topics.is_empty() && listed.enabled);
    let gone = env
        .window_error(Call::NewTopicsGet(NewTopicsGetParams {
            project: root.display().to_string(),
            id: b.topic_id,
        }))
        .await;
    assert_eq!(gone.kind(), Some(ErrorKind::StaleRef));
    env.worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn topic_methods_are_window_only_and_need_an_exact_project() {
    let project = "C:/проект";
    let calls = [
        Call::NewTopicsList(NewTopicsListParams {
            project: project.into(),
        }),
        Call::NewTopicsGet(NewTopicsGetParams {
            project: project.into(),
            id: 1,
        }),
        Call::NewTopicsMarkMissing(NewTopicsMarkMissingParams {
            project: project.into(),
            request_id: "r".into(),
            query: "q".into(),
            modules: vec![],
            files: vec![],
        }),
        Call::NewTopicsCheck(NewTopicsCheckParams {
            project: project.into(),
            id: 1,
            revision: 1,
            expand: false,
        }),
        Call::NewTopicsSetStatus(NewTopicsSetStatusParams {
            project: project.into(),
            id: 1,
            revision: 1,
            status: "open".into(),
        }),
        Call::NewTopicsRename(NewTopicsRenameParams {
            project: project.into(),
            id: 1,
            revision: 1,
            title: "t".into(),
        }),
        Call::NewTopicsDelete(NewTopicsDeleteParams {
            project: project.into(),
            id: 1,
            revision: 1,
        }),
        Call::NewTopicsClear(NewTopicsClearParams {
            project: project.into(),
            confirmed: true,
        }),
        Call::NewTopicsSettings(NewTopicsSettingsParams {
            project: project.into(),
            enabled: true,
        }),
    ];
    for call in &calls {
        assert!(call.window_only(), "{}", call.method());
        assert!(!call.allowed_for(ClientKind::Mcp), "{}", call.method());
        assert!(!call.allowed_for(ClientKind::Cli), "{}", call.method());
    }
    // Окно вызывает обработчик напрямую; проект, которого нет среди зарегистрированных, отклоняется.
    let env = Env::new();
    for call in calls {
        assert!(
            env.handler.dispatch(call.clone()).await.is_err(),
            "{}",
            call.method()
        );
    }
    let relative = env
        .window_error(Call::NewTopicsList(NewTopicsListParams {
            project: "proj".into(),
        }))
        .await;
    assert_eq!(relative.kind(), Some(ErrorKind::InvalidParams));
    env.worker.shutdown().await;
}
