//! Обнаружение worktree, исключение их папок и хук без регистрации
//! (`docs/features/projects/worktrees.md`, Т-48, шаг 6.3.1).

use super::*;

/// Основной checkout `.git` как каталог.
fn git_dir(project: &Path) {
    fs::create_dir_all(project.join(".git")).unwrap();
}

/// Запись `.git/worktrees/<name>` репозитория `repo` и рабочая копия `work` с `.git`-файлом и
/// документом `x.md` в корне.
fn add_worktree(repo: &Path, name: &str, work: &Path, head: &str) {
    let admin = repo.join(".git").join("worktrees").join(name);
    fs::create_dir_all(&admin).unwrap();
    fs::create_dir_all(work).unwrap();
    fs::write(admin.join("HEAD"), head).unwrap();
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
    fs::write(work.join("x.md"), DOC).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn worktree_discovery_creates_no_delta() {
    let env = Env::new();
    let main = env.project("main", &[("a.md", DOC)]);
    git_dir(&main);
    let work = env.projects.path().join("runs").join("feat-x");
    add_worktree(&main, "feat-x", &work, "ref: refs/heads/feat/x\n");
    let worker = env.start();
    worker.register(&main).unwrap();
    idle(&worker).await;

    let linked = worker.linked_worktrees(&main);
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].name, "feat-x");
    assert_eq!(linked[0].branch.as_deref(), Some("feat/x"));
    // Дельты нет: обнаружение ничего не пишет на диск (ТР-1, ТР-2).
    assert!(!main.join(".bars").join("worktrees").exists());
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hook_in_a_worktree_registers_neither_copy_nor_unregistered_main() {
    let env = Env::new();
    let main = env.project("main", &[("a.md", DOC)]);
    git_dir(&main);
    let work = env.projects.path().join("runs").join("feat-x");
    add_worktree(&main, "feat-x", &work, "ref: refs/heads/x\n");
    fs::create_dir_all(work.join("docs")).unwrap();
    fs::write(work.join("docs/y.md"), DOC).unwrap();
    let worker = env.start();

    assert_eq!(worker.session_start(&work).unwrap(), None);
    assert_eq!(worker.session_start(&work.join("docs")).unwrap(), None);
    assert!(worker.projects().is_empty(), "реестр не менялся");

    // Владелец зарегистрирован: хук в worktree даёт владельца и перечитывает его worktree.
    worker.register(&main).unwrap();
    idle(&worker).await;
    assert_eq!(
        worker.session_start(&work.join("docs")).unwrap(),
        Some(main.clone())
    );
    assert_eq!(worker.projects().len(), 1);
    let (project, view) = worker.resolve_cwd(&work.join("docs")).unwrap();
    assert_eq!(project, main);
    assert_eq!(view.unwrap().name, "feat-x");
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn worktree_folder_inside_a_project_is_excluded_and_returns_with_its_record() {
    let env = Env::new();
    let main = env.project("main", &[("a.md", DOC)]);
    git_dir(&main);
    // Папка без точки внутри корня документации проекта: `docs/wt-foo`.
    let work = main.join("docs").join("wt-foo");
    add_worktree(&main, "wt-foo", &work, "ref: refs/heads/foo\n");
    let worker = env.start();
    worker.register(&main).unwrap();
    idle(&worker).await;
    assert_eq!(documents(&main), 1, "документы worktree в индекс не попали");
    assert!(
        bars_core::indexing::nested::excluded(&main)
            .iter()
            .any(|dir| dir == "docs/wt-foo")
    );
    // Исключённая папка — не вложенный проект.
    assert!(worker.nested_projects(&main, false).is_empty());

    // `git worktree remove` без удаления папки: запись пропала, папка возвращается индексу.
    fs::remove_dir_all(main.join(".git").join("worktrees").join("wt-foo")).unwrap();
    worker.rediscover_worktrees(&main);
    idle(&worker).await;
    assert!(worker.linked_worktrees(&main).is_empty());
    assert_eq!(documents(&main), 2);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_wt_is_rediscovered_once_and_then_lists_worktrees() {
    let env = Env::new();
    let main = env.project("main", &[("a.md", DOC)]);
    git_dir(&main);
    let first = env.projects.path().join("runs").join("one");
    add_worktree(&main, "one", &first, "ref: refs/heads/one\n");
    let worker = env.start();
    worker.register(&main).unwrap();
    idle(&worker).await;

    assert_eq!(
        worker
            .find_worktree(&main, WtSelector::parse("one"))
            .unwrap()
            .name,
        "one"
    );
    // По абсолютному пути: W и папка внутри неё.
    let inside = first.join("sub");
    assert!(
        worker
            .find_worktree(&main, WtSelector::parse(&inside.to_string_lossy()))
            .is_ok()
    );
    // Имя ветки как значение не принимается: ошибка со списком.
    let list = worker
        .find_worktree(&main, WtSelector::parse("feat/one"))
        .unwrap_err();
    assert_eq!(list.len(), 1);
    // Worktree создан после обнаружения: повторное чтение при промахе его находит.
    let second = env.projects.path().join("runs").join("two");
    add_worktree(&main, "two", &second, "ref: refs/heads/two\n");
    assert_eq!(
        worker
            .find_worktree(&main, WtSelector::parse("two"))
            .unwrap()
            .name,
        "two"
    );
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lazy_subproject_owner_has_visible_worktrees_without_an_index() {
    let env = Env::new();
    let main = env.project("main", &[("index.md", DOC)]);
    let worker_repo = main.join("backend").join("worker");
    fs::create_dir_all(worker_repo.join("docs")).unwrap();
    fs::write(worker_repo.join("docs/x.md"), DOC).unwrap();
    git_dir(&worker_repo);
    let work = main.join("backend").join("worker-wt-quorum");
    add_worktree(
        &worker_repo,
        "worker-wt-quorum",
        &work,
        "ref: refs/heads/q\n",
    );
    let runtime = env.start();
    runtime.register(&main).unwrap();
    idle(&runtime).await;
    runtime.inner.discover_chain(&main);
    runtime.inner.discover_worktrees(None);

    assert!(runtime.inner.is_lazy(&worker_repo));
    let linked = runtime.linked_worktrees(&worker_repo);
    assert_eq!(linked.len(), 1, "worktree ленивого владельца обнаружены");
    assert!(
        runtime.linked_worktrees(&main).is_empty(),
        "внешний их не получает"
    );
    assert!(!worker_repo.join(".bars").exists(), "заданий и индекса нет");
    runtime.shutdown().await;
}
