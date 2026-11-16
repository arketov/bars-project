//! Поиск и чтение по виду worktree (`docs/features/projects/worktrees.md`, «Поиск по worktree»,
//! «Чтение, структура, точный поиск и содержимое папки», «Чего в worktree нет»). Тесты не
//! запускались: сборка запрещена в этом шаге.

use super::*;
use crate::handler::WorkerHandler;
use crate::worker::Worker;
use bars_protocol::{DeltaState, DiskLinksParams, DiskTextParams, GraphStatusParams, LikeParams};

/// Основной checkout `main` и worktree `feat-x` вне проекта: `docs/guide.md` у обоих разный,
/// `docs/only-wt.md` есть только в worktree.
pub(super) fn repo_with_worktree(projects: &Path) -> (PathBuf, PathBuf) {
    let main = projects.join("main");
    fs::create_dir_all(main.join("docs")).unwrap();
    fs::write(
        main.join("docs/guide.md"),
        "# Руководство\n\nТекст основного.\n",
    )
    .unwrap();
    let admin = main.join(".git/worktrees/feat-x");
    fs::create_dir_all(&admin).unwrap();
    let work = projects.join("runs").join("feat-x");
    fs::create_dir_all(work.join("docs")).unwrap();
    fs::write(
        work.join("docs/guide.md"),
        "# Руководство\n\n## Раздел worktree\n\nТекст worktree.\n",
    )
    .unwrap();
    fs::write(
        work.join("docs/only-wt.md"),
        "# Только в worktree\n\nzqonlyworktreeword\n",
    )
    .unwrap();
    fs::write(admin.join("HEAD"), "ref: refs/heads/feat/x\n").unwrap();
    fs::write(admin.join("commondir"), "../..\n").unwrap();
    fs::write(
        admin.join("gitdir"),
        work.join(".git").to_string_lossy().replace('\\', "/"),
    )
    .unwrap();
    fs::write(
        work.join(".git"),
        format!("gitdir: {}\n", admin.to_string_lossy().replace('\\', "/")),
    )
    .unwrap();
    (main, work)
}

pub(super) fn handler(home: &TempDir) -> WorkerHandler {
    let config = WorkerConfig::new(home.path().to_path_buf()).without_graph_pause();
    let worker = Worker::start(config, FakeProvider::new("m1")).unwrap();
    WorkerHandler::new(worker)
}

pub(super) fn target_wt(cwd: &Path, wt: Option<&str>) -> Target {
    Target {
        cwd: cwd.display().to_string(),
        project: None,
        wt: wt.map(str::to_string),
    }
}

pub(super) async fn wait_idle(worker: &Worker) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !worker.is_idle() {
        assert!(Instant::now() < deadline, "очередь не опустела");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

pub(super) async fn wait_delta(worker: &Worker, main: &Path) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while worker.worktree_delta_status(main, "feat-x").state != DeltaState::Ready
        || !worker.is_idle()
    {
        assert!(Instant::now() < deadline, "дельта не построена");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn read_params(target: Target, path: &str) -> ReadParams {
    ReadParams {
        target,
        path: path.into(),
        lines: None,
        under: None,
        max_lines: None,
        order: None,
        cursor: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_search_with_wt_builds_delta() {
    let home = TempDir::new().unwrap();
    let projects = TempDir::new().unwrap();
    let (main, _work) = repo_with_worktree(projects.path());
    let handler = handler(&home);
    let worker = handler.worker();
    worker.register(&main).unwrap();
    wait_idle(worker).await;
    assert_eq!(
        worker.worktree_delta_status(&main, "feat-x").state,
        DeltaState::None
    );

    // `wt` по имени из основного checkout — первое обращение к виду.
    let mut params = search(&main, "руководство");
    params.target = target_wt(&main, Some("feat-x"));
    let _ = handler.search(params).await;
    wait_delta(worker, &main).await;
    assert!(
        bars_core::indexing::worktree_delta::delta_path(&main, "feat-x").is_file(),
        "дельта лежит в .bars/worktrees/<имя>"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn search_while_building_answers_from_owner_with_note() {
    let home = TempDir::new().unwrap();
    let projects = TempDir::new().unwrap();
    let (main, _work) = repo_with_worktree(projects.path());
    let handler = handler(&home);
    let worker = handler.worker();
    worker.register(&main).unwrap();
    wait_idle(worker).await;

    // Пауза очереди замораживает и сверку: дельта не успевает достроиться.
    handler.index_pause();
    let mut params = search(&main, "руководство");
    params.target = target_wt(&main, Some("feat-x"));
    let output = handler.search(params.clone()).await.unwrap();
    let info = output
        .applied
        .scope
        .worktree
        .as_ref()
        .expect("в applied.scope виден вид");
    assert_eq!(info.name, "feat-x");
    assert_eq!(info.branch.as_deref(), Some("feat/x"));
    assert_ne!(info.state, DeltaState::Ready);
    let note = info.note.as_deref().expect("пометка над результатами");
    assert!(
        note.contains("feat-x") && note.contains("основному checkout"),
        "{note}"
    );
    assert!(
        output.results.iter().any(|hit| hit.path == "docs/guide.md"),
        "ответ идёт по владельцу"
    );
    // Версии worktree в ответе владельца нет.
    assert!(
        output
            .results
            .iter()
            .all(|hit| !hit.text.contains("Раздел worktree"))
    );

    // Дельта достроилась: слияние, пометки нет, новый документ виден только с `wt`.
    handler.index_resume();
    wait_delta(worker, &main).await;
    let mut only = search(&main, "zqonlyworktreeword");
    only.target = target_wt(&main, Some("feat-x"));
    let merged = handler.search(only).await.unwrap();
    let info = merged.applied.scope.worktree.as_ref().unwrap();
    assert_eq!(info.state, DeltaState::Ready);
    assert!(
        info.delta_documents >= 2,
        "guide.md изменён, only-wt.md новый: {info:?}"
    );
    assert!(
        merged
            .results
            .iter()
            .any(|hit| hit.path == "docs/only-wt.md")
    );
    let from_main = handler
        .search(search(&main, "zqonlyworktreeword"))
        .await
        .unwrap();
    assert!(from_main.applied.scope.worktree.is_none());
    assert!(
        from_main
            .results
            .iter()
            .all(|hit| hit.path != "docs/only-wt.md")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn read_outline_occurrences_use_worktree_disk_without_delta() {
    let home = TempDir::new().unwrap();
    let projects = TempDir::new().unwrap();
    let (main, work) = repo_with_worktree(projects.path());
    let handler = handler(&home);
    let worker = handler.worker();
    worker.register(&main).unwrap();
    wait_idle(worker).await;
    let wt = target_wt(&main, Some("feat-x"));

    // Чтение: текст и ревизия — из файла W, и документ, которого у владельца нет.
    let read = handler
        .read(read_params(wt.clone(), "docs/guide.md"))
        .unwrap();
    assert!(read.text.contains("Текст worktree"), "{}", read.text);
    let disk = fs::read_to_string(work.join("docs/guide.md")).unwrap();
    assert_eq!(
        read.revision,
        blake3::hash(disk.as_bytes()).to_hex().to_string()
    );
    let only = handler.read(read_params(wt.clone(), "only-wt.md")).unwrap();
    assert!(only.text.contains("zqonlyworktreeword"));
    // Основной checkout читает свой файл.
    let main_read = handler
        .read(read_params(target_wt(&main, Some(".")), "docs/guide.md"))
        .unwrap();
    assert!(main_read.text.contains("Текст основного"));
    // Допуск путей — от корня вида: выход наружу не проходит.
    assert!(
        handler
            .read(read_params(wt.clone(), "../../main/docs/guide.md"))
            .is_err()
    );

    // Структура из файла W.
    let outline = handler
        .outline(OutlineParams {
            target: wt.clone(),
            path: "docs/guide.md".into(),
            levels: vec![],
            under: None,
            under_line: None,
        })
        .unwrap();
    assert!(
        outline
            .headings
            .iter()
            .any(|heading| heading.text == "Раздел worktree")
    );

    // Точный поиск обходит корень вида.
    let found = handler
        .disk_text(DiskTextParams {
            target: wt.clone(),
            query: "Текст worktree".into(),
            case_sensitive: false,
            regex: false,
            path_filter: String::new(),
            modules: vec![],
            files: vec![],
            paths: vec![],
            limit_docs: None,
            limit_hits: None,
            path: None,
            offset: 0,
            revision: None,
        })
        .await
        .unwrap();
    assert_eq!(found.result.documents.len(), 1);
    assert_eq!(found.result.documents[0].path, "docs/guide.md");

    // Содержимое папки — по диску W.
    let listing = handler
        .read_folder(read_params(wt.clone(), "docs/"))
        .await
        .unwrap()
        .folder
        .unwrap();
    let names: Vec<&str> = listing
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert!(names.contains(&"only-wt.md"), "{names:?}");
    assert!(names.contains(&"guide.md"), "{names:?}");

    // Дельту эти вызовы не создают.
    assert!(!main.join(".bars").join("worktrees").exists());
    assert_eq!(
        worker.worktree_delta_status(&main, "feat-x").state,
        DeltaState::None
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn graph_audit_links_answer_from_owner_with_note() {
    let home = TempDir::new().unwrap();
    let projects = TempDir::new().unwrap();
    let (main, _work) = repo_with_worktree(projects.path());
    let handler = handler(&home);
    let worker = handler.worker();
    worker.register(&main).unwrap();
    wait_idle(worker).await;

    // Граф и ссылки по worktree: ответ владельца и строка пометки.
    let graph = handler
        .dispatch(Call::GraphStatus(GraphStatusParams {
            target: target_wt(&main, Some("feat-x")),
        }))
        .await
        .unwrap();
    let note = graph["worktree_note"].as_str().expect("пометка у графа");
    assert!(
        note.contains("feat-x") && note.contains("только у основного checkout"),
        "{note}"
    );
    let links = handler
        .dispatch(Call::DiskLinks(DiskLinksParams {
            target: target_wt(&main, Some("feat-x")),
            path: "docs/guide.md".into(),
            direction: "outgoing".into(),
            limit: None,
            cursor: None,
        }))
        .await
        .unwrap();
    assert!(links["worktree_note"].is_string(), "пометка у ссылок");

    // Основной checkout пометки не получает.
    let plain = handler
        .dispatch(Call::GraphStatus(GraphStatusParams {
            target: target_wt(&main, Some(".")),
        }))
        .await
        .unwrap();
    assert!(plain.get("worktree_note").is_none());
    // Дельту ни граф, ни ссылки не создают.
    assert!(!main.join(".bars").join("worktrees").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn similar_duplicates_with_worktree_are_unsupported() {
    let home = TempDir::new().unwrap();
    let projects = TempDir::new().unwrap();
    let (main, work) = repo_with_worktree(projects.path());
    let handler = handler(&home);
    let worker = handler.worker();
    worker.register(&main).unwrap();
    wait_idle(worker).await;

    for target in [
        target_wt(&main, Some("feat-x")),
        // Умолчание из cwd в worktree даёт тот же вид.
        target_wt(&work.join("docs"), None),
    ] {
        let error = handler
            .similar(SimilarParams {
                target: target.clone(),
                chunk_id: "1-00000000".into(),
                modules: vec![],
                paths: vec![],
                exclude_document: false,
                limit: None,
            })
            .await
            .unwrap_err();
        assert_eq!(error.kind(), Some(ErrorKind::InvalidParams));
        assert_eq!(error.data.as_ref().unwrap()["kind"], "worktree_unsupported");
        assert!(error.message.contains("wt=\".\""), "{}", error.message);
        let error = handler
            .duplicates(DuplicatesParams {
                target,
                modules: vec![],
                limit: None,
            })
            .await
            .unwrap_err();
        assert_eq!(error.data.as_ref().unwrap()["kind"], "worktree_unsupported");
    }
}

fn like_params(target: Target, source: &str) -> LikeParams {
    LikeParams {
        target,
        source: source.into(),
        paths: vec![],
        limit: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn like_in_worktree_finds_source_in_delta_and_marks_links() {
    let home = TempDir::new().unwrap();
    let projects = TempDir::new().unwrap();
    let (main, work) = repo_with_worktree(projects.path());
    let handler = handler(&home);
    let worker = handler.worker();
    worker.register(&main).unwrap();
    wait_idle(worker).await;
    // Первое обращение к виду строит дельту.
    let mut first = search(&main, "руководство");
    first.target = target_wt(&main, Some("feat-x"));
    let _ = handler.search(first).await;
    wait_delta(worker, &main).await;

    let wt = target_wt(&main, Some("feat-x"));
    // Строки 5 у владельца нет (там три строки): источник — чанк дельты.
    let output = handler
        .like(like_params(wt.clone(), "docs/guide.md:5"))
        .await
        .unwrap();
    assert_eq!(output.source.path, "docs/guide.md");
    assert!(
        output.source.text.contains("Текст worktree"),
        "{}",
        output.source.text
    );
    assert!(output.results.iter().all(|hit| hit.path != "docs/guide.md"));
    let note = output.worktree_note.as_deref().expect("пометка про ссылки");
    assert!(
        note.contains("feat-x") && note.contains("входящие ссылки"),
        "{note}"
    );
    // Документ, созданный только в worktree, тоже источник.
    let only = handler
        .like(like_params(wt.clone(), "docs/only-wt.md:1"))
        .await
        .unwrap();
    assert_eq!(only.source.path, "docs/only-wt.md");
    // ID чанка из выдачи по виду принимается как есть.
    let by_id = handler
        .like(like_params(wt, &output.source.chunk_id))
        .await
        .unwrap();
    assert_eq!(by_id.source.chunk_id, output.source.chunk_id);
    // Умолчание из cwd в worktree даёт тот же вид.
    let from_cwd = handler
        .like(like_params(
            target_wt(&work.join("docs"), None),
            "docs/guide.md:5",
        ))
        .await
        .unwrap();
    assert_eq!(from_cwd.source.chunk_id, output.source.chunk_id);
    // Основной checkout: версия владельца и без пометки.
    let missing = handler
        .like(like_params(target_wt(&main, Some(".")), "docs/guide.md:5"))
        .await
        .unwrap_err();
    assert_ne!(
        missing.data.as_ref().and_then(|data| data["kind"].as_str()),
        Some("worktree_unsupported")
    );
    let plain = handler
        .like(like_params(target_wt(&main, Some(".")), "docs/guide.md:3"))
        .await
        .unwrap();
    assert!(plain.worktree_note.is_none());
    assert!(plain.source.text.contains("Текст основного"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn like_while_delta_builds_answers_from_owner_with_note() {
    let home = TempDir::new().unwrap();
    let projects = TempDir::new().unwrap();
    let (main, _work) = repo_with_worktree(projects.path());
    let handler = handler(&home);
    let worker = handler.worker();
    worker.register(&main).unwrap();
    wait_idle(worker).await;

    // Пауза очереди замораживает и сверку: дельта не успевает достроиться.
    handler.index_pause();
    let wt = target_wt(&main, Some("feat-x"));
    let output = handler
        .like(like_params(wt.clone(), "docs/guide.md:3"))
        .await
        .unwrap();
    assert!(
        output.source.text.contains("Текст основного"),
        "ответ идёт по владельцу: {}",
        output.source.text
    );
    let note = output.worktree_note.as_deref().expect("пометка");
    assert!(
        note.contains("основному checkout") && note.contains("входящие ссылки"),
        "{note}"
    );
    // Документа worktree у владельца нет: ошибка называет причину.
    let error = handler
        .like(like_params(wt, "docs/only-wt.md:1"))
        .await
        .unwrap_err();
    assert_eq!(error.kind(), Some(ErrorKind::InvalidParams));
    assert!(
        error.message.contains("основному checkout"),
        "{}",
        error.message
    );
    handler.index_resume();
    wait_delta(worker, &main).await;
}
