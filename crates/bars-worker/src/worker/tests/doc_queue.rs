//! Очередь документов в воркере: пауза после правки, явная команда, удаление, остановка
//! (`docs/features/indexing/document-queue.md`, Т-41, Т-42).

use super::*;
use std::sync::atomic::Ordering as AtomicOrdering;

fn started(env: &Env, pause: Duration) -> Worker {
    let mut config = env.config();
    config.edit_pause = Some(pause);
    // Хвостовая полная проверка не должна вмешиваться в замер паузы.
    config.tail_delay = Duration::from_secs(60);
    Worker::start(config, env.provider.clone()).unwrap()
}

fn embed_calls(env: &Env) -> usize {
    env.provider
        .counters
        .embed_calls
        .load(AtomicOrdering::SeqCst)
}

fn titles(project: &Path) -> Vec<String> {
    let index = Index::open(&index_path(project), Journal::Wal).unwrap();
    let mut statement = index
        .conn()
        .prepare("SELECT title FROM documents ORDER BY path")
        .unwrap();
    let titles = statement
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    drop(statement);
    index.close().unwrap();
    titles
}

fn doc(title: &str) -> String {
    format!(
        "---\ntitle: {title}\n---\n\n# {title}\n\nТекст раздела про очередь документов, версия {title}.\n"
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stream_of_edits_embeds_the_document_once_after_the_pause() {
    let env = Env::new();
    let project = env.project("alpha", &[("a.md", DOC)]);
    let worker = started(&env, Duration::from_millis(1500));
    worker.register(&project).unwrap();
    idle(&worker).await;
    let before = embed_calls(&env);

    let file = project.join("docs/a.md");
    for version in 1..=5 {
        fs::write(&file, doc(&format!("Правка {version}"))).unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    wait_until("документ ждёт паузы", || {
        worker
            .doc_queue(&project, 10)
            .is_ok_and(|(items, _)| items.iter().any(|item| item.state == DocState::Waiting))
    })
    .await;
    assert_eq!(
        embed_calls(&env),
        before,
        "пока идёт пауза, embed не вызывается"
    );
    let status = worker.status(&project).unwrap();
    assert_eq!(status.pending, 1);
    assert!(!worker.is_idle());

    idle(&worker).await;
    assert_eq!(embed_calls(&env), before + 1, "один embed на серию правок");
    assert_eq!(titles(&project), vec!["Правка 5".to_string()]);
    assert_eq!(worker.status(&project).unwrap().pending, 0);
    assert!(worker.doc_queue(&project, 10).unwrap().0.is_empty());
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_explicit_check_does_not_wait_for_the_pause() {
    let env = Env::new();
    let project = env.project("alpha", &[("a.md", DOC)]);
    let worker = started(&env, Duration::from_secs(120));
    worker.register(&project).unwrap();
    idle(&worker).await;

    let file = project.join("docs/a.md");
    fs::write(&file, doc("Явная")).unwrap();
    worker
        .documents_edited(&project, vec![file.clone()])
        .unwrap();
    let (items, counts) = worker.doc_queue(&project, 10).unwrap();
    assert_eq!(counts.waiting, 1);
    assert_eq!(items[0].path, "docs/a.md");
    assert_eq!(items[0].state, DocState::Waiting);
    assert!(
        items[0]
            .due_in
            .is_some_and(|left| left > Duration::from_secs(60))
    );

    worker.check_files(&project, vec![file]).unwrap();
    // Пауза в 120 секунд не мешает: документ в индексе сразу. Позднее эхо слежения за тем же
    // файлом может снова поставить запись, поэтому пустоты очереди здесь не требуется.
    wait_until(
        "документ проиндексирован без паузы",
        || titles(&project) == vec!["Явная".to_string()],
    )
    .await;
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_waiting_document_survives_a_full_check_until_its_pause_ends() {
    let env = Env::new();
    let project = env.project("alpha", &[("a.md", DOC), ("b.md", DOC)]);
    let worker = started(&env, Duration::from_secs(120));
    worker.register(&project).unwrap();
    idle(&worker).await;

    fs::write(project.join("docs/a.md"), doc("Ждёт")).unwrap();
    worker
        .documents_edited(&project, vec![project.join("docs/a.md")])
        .unwrap();
    // Полная проверка находит правку, но документ ждёт паузы и остаётся как был.
    worker.check_all();
    wait_until(
        "проверка пропустила документ",
        || {
            worker
                .status(&project)
                .ok()
                .and_then(|status| status.last_report)
                .is_some_and(|report| report.deferred == 1)
        },
    )
    .await;
    assert!(!titles(&project).contains(&"Ждёт".to_string()));
    assert_eq!(worker.doc_queue(&project, 10).unwrap().1.waiting, 1);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deleted_document_leaves_the_index_without_a_pause() {
    let env = Env::new();
    let project = env.project("alpha", &[("a.md", DOC), ("b.md", DOC)]);
    let worker = started(&env, Duration::from_secs(120));
    worker.register(&project).unwrap();
    idle(&worker).await;
    assert_eq!(documents(&project), 2);

    fs::remove_file(project.join("docs/b.md")).unwrap();
    wait_until("документ убран из индекса", || {
        documents(&project) == 1
    })
    .await;
    assert!(worker.doc_queue(&project, 10).unwrap().0.is_empty());
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_records_are_created_while_the_project_waits_for_a_reindex() {
    let env = Env::new();
    let project = env.project("alpha", &[("a.md", DOC)]);
    let worker = started(&env, Duration::from_millis(50));
    worker.register(&project).unwrap();
    idle(&worker).await;
    env.provider.set_model("m2");
    worker.check_all();
    wait_until("нужна переиндексация", || {
        state(&worker, &project) == ProjectState::ReindexRequired
    })
    .await;

    fs::write(project.join("docs/a.md"), doc("После смены модели")).unwrap();
    worker
        .documents_edited(&project, vec![project.join("docs/a.md")])
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(worker.doc_queue(&project, 10).unwrap().1.total(), 0);
    assert_eq!(state(&worker, &project), ProjectState::ReindexRequired);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stopping_a_project_drops_its_records() {
    let env = Env::new();
    let project = env.project("alpha", &[("a.md", DOC)]);
    let worker = started(&env, Duration::from_secs(120));
    worker.register(&project).unwrap();
    idle(&worker).await;

    fs::write(project.join("docs/a.md"), doc("Остановка")).unwrap();
    worker
        .documents_edited(&project, vec![project.join("docs/a.md")])
        .unwrap();
    assert_eq!(worker.doc_queue(&project, 10).unwrap().1.waiting, 1);
    assert!(worker.cancel(&project).unwrap());
    assert_eq!(worker.doc_queue(&project, 10).unwrap().1.total(), 0);
    worker.shutdown().await;
}
