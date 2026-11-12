//! Автопоиск подпроектов, ленивый индекс и уборка (`docs/features/projects/subprojects.md`, Т-49).

use super::*;

/// Репозиторий: каталог `.git` и, при `docs`, `docs/x.md`.
fn repo(root: &Path, rel: &str, docs: bool) -> PathBuf {
    let path = rel
        .split('/')
        .fold(root.to_path_buf(), |dir, part| dir.join(part));
    fs::create_dir_all(path.join(".git")).unwrap();
    if docs {
        fs::create_dir_all(path.join("docs")).unwrap();
        fs::write(path.join("docs/x.md"), DOC).unwrap();
    }
    path
}

/// Главный проект по образцу `tmc-opovechenie`: четыре подпроекта, `deploy` без `docs/` и
/// worktree рядом с `backend/api`.
fn sample(env: &Env) -> PathBuf {
    let main = env.project("tmc-opovechenie", &[("index.md", DOC)]);
    repo(&main, "backend/api", true);
    repo(&main, "backend/worker", true);
    repo(&main, "frontend/web-angular", true);
    repo(&main, "frontend/web-design", true);
    repo(&main, "deploy", false);
    let wt = main.join("backend").join("api-wt-quorum");
    fs::create_dir_all(wt.join("docs")).unwrap();
    fs::write(wt.join(".git"), "gitdir: /nowhere/.git/worktrees/wt\n").unwrap();
    fs::write(wt.join("docs/x.md"), DOC).unwrap();
    main
}

fn api_of(main: &Path) -> PathBuf {
    main.join("backend").join("api")
}

fn web_of(main: &Path) -> PathBuf {
    main.join("frontend").join("web-angular")
}

/// Воркер с зарегистрированным главным проектом и завершённым автопоиском.
async fn discovered(env: &Env) -> (Worker, PathBuf) {
    let main = sample(env);
    let worker = env.start();
    worker.register(&main).unwrap();
    idle(&worker).await;
    // Фоновый обход мог уже идти: проход один на процесс, повтор ничего не меняет.
    worker.inner.discover_chain(&main);
    (worker, main)
}

fn entry(worker: &Worker, path: &Path) -> Option<ProjectEntry> {
    lock(&worker.inner.registry).find(path).cloned()
}

fn registry_text(env: &Env) -> String {
    fs::read_to_string(env.home.path().join("projects.toml")).unwrap_or_default()
}

fn subproject_paths(worker: &Worker, main: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = worker
        .projects()
        .into_iter()
        .filter(|project| project.parent.as_deref() == Some(main))
        .map(|project| project.path)
        .collect();
    paths.sort();
    paths
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discovery_registers_four_lazy_subprojects_and_leaves_their_folders_alone() {
    let env = Env::new();
    let (worker, main) = discovered(&env).await;
    let found = subproject_paths(&worker, &main);
    assert_eq!(
        found,
        vec![
            api_of(&main),
            main.join("backend").join("worker"),
            web_of(&main),
            main.join("frontend").join("web-design"),
        ]
    );
    for path in &found {
        let entry = entry(&worker, path).unwrap();
        assert!(entry.discovered && entry.lazy);
        assert_eq!(state(&worker, path), ProjectState::Lazy);
        assert!(!path.join(".bars").exists(), "{}", path.display());
        assert!(!worker.status(path).unwrap().watching);
        assert!(!worker.status(path).unwrap().queued);
    }
    idle(&worker).await;
    assert!(worker.queued().is_empty());
    let listed = worker.projects();
    let api = listed.iter().find(|p| p.path == api_of(&main)).unwrap();
    assert_eq!(
        api.qualified_name.as_deref(),
        Some("tmc-opovechenie/backend/api")
    );
    assert!(api.lazy && api.discovered);
    // Главный индекс не менялся: подпроекты лежат вне его корня документации.
    assert_eq!(documents(&main), 1);
    let text = registry_text(&env);
    assert!(text.contains("discovered = true") && text.contains("lazy = true"));
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_access_by_the_composite_name_builds_the_index() {
    let env = Env::new();
    let (worker, main) = discovered(&env).await;
    let api = worker.resolve("tmc-opovechenie/backend/api").unwrap();
    assert_eq!(api, api_of(&main));
    assert!(entry(&worker, &api).unwrap().lazy, "разбор имени не будит");
    worker.observe_mcp(&api).unwrap();
    let woke = entry(&worker, &api).unwrap();
    assert!(woke.discovered && !woke.lazy);
    let config = fs::read_to_string(api.join(".bars").join("config.toml")).unwrap();
    assert!(config.contains("docs_root = \"docs\""), "{config}");
    wait_until("индекс подпроекта", || {
        api.join(".bars").join("index.sqlite").is_file() && worker.is_idle()
    })
    .await;
    // Файл инструкции агента тоже документ: хотя бы `x.md` в индексе.
    assert!(documents(&api) >= 1);
    assert_ne!(state(&worker, &api), ProjectState::Lazy);
    // Повторное обращение реестр не переписывает.
    let before = registry_text(&env);
    worker.observe_mcp(&api).unwrap();
    assert_eq!(registry_text(&env), before);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn passive_calls_and_the_hourly_check_do_not_wake_a_lazy_project() {
    let env = Env::new();
    let (worker, main) = discovered(&env).await;
    let api = api_of(&main);
    worker.observe_mcp_with(&api, true).unwrap();
    assert!(entry(&worker, &api).unwrap().lazy);
    assert!(lock(&worker.inner.projects)[&api].mcp_until.is_none());
    let _ = worker.projects();
    worker.check_all();
    worker.inner.discover_all();
    idle(&worker).await;
    assert!(entry(&worker, &api).unwrap().lazy);
    assert_eq!(state(&worker, &api), ProjectState::Lazy);
    assert!(!api.join(".bars").exists());
    assert!(worker.queued().is_empty());
    // Пассивный вызов по обычному проекту ведёт себя как раньше.
    worker.observe_mcp_with(&main, true).unwrap();
    assert!(lock(&worker.inner.projects)[&main].mcp_until.is_some());
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn opening_a_lazy_subproject_in_a_window_wakes_it() {
    let env = Env::new();
    let (worker, main) = discovered(&env).await;
    let api = api_of(&main);
    worker.observe_window("main", Some(&main)).unwrap();
    assert!(
        entry(&worker, &api).unwrap().lazy,
        "главный не будит подпроекты"
    );
    worker.observe_window("main", Some(&api)).unwrap();
    assert!(!entry(&worker, &api).unwrap().lazy);
    assert!(api.join(".bars").join("config.toml").is_file());
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unknown_subproject_is_registered_lazy_by_name_or_by_path() {
    let env = Env::new();
    let main = sample(&env);
    let worker = env.start();
    worker.register(&main).unwrap();
    idle(&worker).await;
    // Обход мог ещё не начаться: селектор регистрирует сам, одной проверкой путей.
    let by_name = worker.resolve("tmc-opovechenie/backend/worker").unwrap();
    assert_eq!(by_name, main.join("backend").join("worker"));
    let by_path = worker
        .resolve(&web_of(&main).display().to_string())
        .unwrap();
    assert_eq!(by_path, web_of(&main));
    for path in [&by_name, &by_path] {
        let entry = entry(&worker, path).unwrap();
        assert!(entry.discovered && entry.lazy);
        assert!(!path.join(".bars").exists());
    }
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_names_explain_themselves() {
    let env = Env::new();
    let (worker, main) = discovered(&env).await;
    // Папка есть, но это не подпроект: репозиторий без `docs/`, worktree.
    for selector in [
        "tmc-opovechenie/deploy",
        "tmc-opovechenie/backend/api-wt-quorum",
    ] {
        let message = match worker.resolve(selector) {
            Err(WorkerError::Resolve(error)) => error.to_string(),
            other => panic!("{selector}: {other:?}"),
        };
        assert!(message.contains("не подпроект"), "{message}");
        assert!(message.contains("bars-cli register"), "{message}");
    }
    // Папки нет: перечень подпроектов главного.
    let message = match worker.resolve("tmc-opovechenie/backend/nope") {
        Err(WorkerError::Resolve(error)) => error.to_string(),
        other => panic!("{other:?}"),
    };
    assert!(message.contains("подпроекты"), "{message}");
    assert!(message.contains("tmc-opovechenie/backend/api"), "{message}");
    assert!(!lock(&worker.inner.registry).contains(&main.join("deploy")));
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_inside_a_subproject_belongs_to_it_and_wakes_it() {
    let env = Env::new();
    let main = sample(&env);
    let worker = env.start();
    worker.register(&main).unwrap();
    idle(&worker).await;
    let api = api_of(&main);
    let cwd = api.join("src").join("deep");
    fs::create_dir_all(&cwd).unwrap();
    // Точечная проверка без обхода: подпроект регистрируется ленивым, индекса нет. Фоновый обход
    // мог найти его раньше, тогда проверка ничего нового не регистрирует.
    worker.register_for_cwd(&cwd);
    assert_eq!(worker.project_for_cwd(&cwd), Some(api.clone()));
    assert!(entry(&worker, &api).unwrap().lazy);
    assert!(!api.join(".bars").exists());
    // Начало сессии — первое обращение.
    assert_eq!(worker.session_start(&cwd).unwrap(), Some(api.clone()));
    assert!(!entry(&worker, &api).unwrap().lazy);
    assert!(api.join(".bars").join("config.toml").is_file());
    assert!(api.join("docs").join("BARS_INSTRUCTION.md").is_file());
    // Сессия в worktree рядом остаётся при главном проекте.
    let wt = main.join("backend").join("api-wt-quorum");
    assert!(worker.register_for_cwd(&wt).is_empty());
    assert!(!lock(&worker.inner.registry).contains(&wt));
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registering_a_discovered_subproject_makes_it_standalone() {
    let env = Env::new();
    let (worker, main) = discovered(&env).await;
    let api = api_of(&main);
    let result = worker.register(&api).unwrap();
    assert!(result.already, "запись одна");
    let entry = entry(&worker, &api).unwrap();
    assert!(!entry.discovered && !entry.lazy);
    assert_eq!(
        lock(&worker.inner.registry)
            .entries()
            .iter()
            .filter(|e| e.path == api)
            .count(),
        1
    );
    idle(&worker).await;
    assert!(documents(&api) >= 1, "индекс один");
    assert_ne!(state(&worker, &api), ProjectState::Lazy);
    let listed = worker.projects();
    let api_row = listed.iter().find(|p| p.path == api).unwrap();
    assert_eq!(api_row.parent.as_deref(), Some(main.as_path()));
    assert!(!api_row.discovered);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_subproject_that_stops_matching_leaves_the_registry_and_keeps_its_files() {
    let env = Env::new();
    let (worker, main) = discovered(&env).await;
    let api = api_of(&main);
    let web = web_of(&main);
    // Самостоятельный: человек зарегистрировал, правило его не касается.
    worker.register(&web).unwrap();
    idle(&worker).await;
    fs::create_dir_all(api.join(".bars")).unwrap();
    fs::remove_dir_all(api.join("docs")).unwrap();
    fs::remove_dir_all(web.join("docs")).unwrap();
    worker.inner.discover_chain(&main);
    assert!(entry(&worker, &api).is_none());
    assert!(api.join(".bars").is_dir());
    assert!(api.join(".git").is_dir());
    assert!(!lock(&worker.inner.projects).contains_key(&api));
    assert!(entry(&worker, &web).is_some(), "самостоятельный остаётся");
    assert!(entry(&worker, &main.join("backend").join("worker")).is_some());
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn removing_the_main_project_removes_its_discovered_subprojects_only() {
    let env = Env::new();
    let (worker, main) = discovered(&env).await;
    let web = web_of(&main);
    worker.register(&web).unwrap();
    idle(&worker).await;
    worker.unregister(&main.display().to_string()).unwrap();
    let left: Vec<PathBuf> = lock(&worker.inner.registry).paths();
    assert_eq!(
        left,
        vec![web.clone()],
        "найденные ушли, самостоятельный остался"
    );
    assert!(web.join(".bars").is_dir());
    assert!(!lock(&worker.inner.projects).contains_key(&api_of(&main)));
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn removing_a_standalone_subproject_that_still_matches_brings_it_back_lazy() {
    let env = Env::new();
    let (worker, main) = discovered(&env).await;
    let api = api_of(&main);
    worker.register(&api).unwrap();
    idle(&worker).await;
    worker.unregister(&api.display().to_string()).unwrap();
    assert!(entry(&worker, &api).is_none());
    worker.inner.discover_chain(&main);
    let back = entry(&worker, &api).unwrap();
    assert!(back.discovered && back.lazy);
    assert!(api.join(".bars").is_dir(), "данные остались");
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unavailable_main_project_keeps_its_discovered_subprojects() {
    let env = Env::new();
    let (worker, main) = discovered(&env).await;
    let before = subproject_paths(&worker, &main);
    assert_eq!(before.len(), 4);
    fs::remove_dir_all(&main).unwrap();
    worker.check_all();
    worker.inner.discover_all();
    assert_eq!(subproject_paths(&worker, &main), before);
    assert_eq!(state(&worker, &main), ProjectState::Unavailable);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn docs_root_dot_of_the_main_project_removes_the_discovered_subprojects() {
    let env = Env::new();
    let (worker, main) = discovered(&env).await;
    let web = web_of(&main);
    worker.register(&web).unwrap();
    idle(&worker).await;
    worker
        .set_docs_root(&main, ".")
        .expect("корень документации `.`");
    worker.inner.discover_chain(&main);
    assert!(subproject_paths(&worker, &main) == vec![web.clone()]);
    assert!(!entry(&worker, &web).unwrap().discovered);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registered_nested_projects_are_not_reindexed_or_rewritten_by_discovery() {
    let env = Env::new();
    let main = sample(&env);
    let web = web_of(&main);
    let worker = env.start();
    worker.register(&web).unwrap();
    worker.register(&main).unwrap();
    idle(&worker).await;
    let index = web.join(".bars").join("index.sqlite");
    let modified = fs::metadata(&index).unwrap().modified().unwrap();
    let config = fs::read_to_string(web.join(".bars").join("config.toml")).unwrap();
    worker.inner.discover_chain(&main);
    worker.check_all();
    idle(&worker).await;
    let kept = entry(&worker, &web).unwrap();
    assert!(!kept.discovered && !kept.lazy);
    assert_eq!(fs::metadata(&index).unwrap().modified().unwrap(), modified);
    assert_eq!(
        fs::read_to_string(web.join(".bars").join("config.toml")).unwrap(),
        config
    );
    // Доступен и своим именем, и составным.
    assert_eq!(worker.resolve("web-angular").unwrap(), web);
    assert_eq!(
        worker
            .resolve("tmc-opovechenie/frontend/web-angular")
            .unwrap(),
        web
    );
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lazy_projects_stay_lazy_across_a_restart() {
    let env = Env::new();
    let (worker, main) = discovered(&env).await;
    worker.shutdown().await;
    let worker = env.start();
    idle(&worker).await;
    let api = api_of(&main);
    assert!(entry(&worker, &api).unwrap().lazy);
    assert_eq!(state(&worker, &api), ProjectState::Lazy);
    assert!(!api.join(".bars").exists());
    assert!(worker.queued().is_empty());
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_access_keeps_an_existing_config_with_its_own_docs_root() {
    let env = Env::new();
    let (worker, main) = discovered(&env).await;
    let api = api_of(&main);
    fs::create_dir_all(api.join(".bars")).unwrap();
    let own = "# мой файл\ndocs_root = \"docs\"\n";
    fs::write(api.join(".bars").join("config.toml"), own).unwrap();
    worker.observe_mcp(&api).unwrap();
    assert!(!entry(&worker, &api).unwrap().lazy);
    assert_eq!(
        fs::read_to_string(api.join(".bars").join("config.toml")).unwrap(),
        own
    );
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deleting_a_lazy_subproject_removes_its_entry() {
    let env = Env::new();
    let (worker, main) = discovered(&env).await;
    let api = api_of(&main);
    worker
        .delete_project("tmc-opovechenie/backend/api")
        .await
        .unwrap();
    assert!(entry(&worker, &api).is_none());
    assert!(api.join("docs").join("x.md").is_file());
    worker.shutdown().await;
}
