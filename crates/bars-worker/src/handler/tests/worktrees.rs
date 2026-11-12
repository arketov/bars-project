//! Вид проекта вызова: `wt`, умолчание из `cwd`, ошибка `worktree_not_found`, часть WORKTREES
//! ответа `status` (`docs/features/projects/worktrees.md`, «Разрешение цели вызова»).

use super::*;
use crate::handler::WorkerHandler;
use crate::worker::{ViewSelector, Worker};

/// Основной checkout `main` (каталог `.git`) и worktree `feat-x` вне проекта.
fn repo_with_worktree(harness_projects: &Path) -> (PathBuf, PathBuf) {
    let main = harness_projects.join("main");
    fs::create_dir_all(main.join("docs")).unwrap();
    fs::write(main.join("docs/guide.md"), "# Руководство\n\nТекст.\n").unwrap();
    let admin = main.join(".git/worktrees/feat-x");
    fs::create_dir_all(&admin).unwrap();
    let work = harness_projects.join("runs").join("feat-x");
    fs::create_dir_all(work.join("docs")).unwrap();
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

fn handler(home: &TempDir) -> WorkerHandler {
    let config = WorkerConfig::new(home.path().to_path_buf()).without_graph_pause();
    let worker = Worker::start(config, FakeProvider::new("m1")).unwrap();
    WorkerHandler::new(worker)
}

fn target_with(cwd: &Path, project: Option<&Path>, wt: Option<&str>) -> Target {
    Target {
        cwd: cwd.display().to_string(),
        project: project.map(|path| path.display().to_string()),
        wt: wt.map(str::to_string),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wt_dot_is_main_checkout_and_unknown_wt_lists_worktrees() {
    let home = TempDir::new().unwrap();
    let projects = TempDir::new().unwrap();
    let (main, work) = repo_with_worktree(projects.path());
    let handler = handler(&home);
    handler.worker().register(&main).unwrap();
    let cwd = work.join("docs");

    // cwd в worktree без `wt` — этот worktree; `wt="."` — основной checkout.
    let (project, view) = handler
        .resolve_view(&target_with(&cwd, None, None))
        .unwrap();
    assert_eq!(project, main);
    assert!(matches!(&view, ViewSelector::Worktree(item) if item.name == "feat-x"));
    let (_, view) = handler
        .resolve_view(&target_with(&cwd, None, Some(".")))
        .unwrap();
    assert_eq!(view, ViewSelector::Main);
    // Пустая строка равна отсутствию.
    let (_, view) = handler
        .resolve_view(&target_with(&cwd, None, Some("  ")))
        .unwrap();
    assert!(matches!(view, ViewSelector::Worktree(_)));
    // По имени и по абсолютному пути из основного checkout.
    for value in ["feat-x".to_string(), work.display().to_string()] {
        let (_, view) = handler
            .resolve_view(&target_with(&main, None, Some(&value)))
            .unwrap();
        assert!(matches!(view, ViewSelector::Worktree(item) if item.name == "feat-x"));
    }

    // Имя ветки — ошибка со списком worktree и ветками.
    let error = handler
        .resolve_view(&target_with(&main, None, Some("feat/x")))
        .unwrap_err();
    assert_eq!(error.kind(), Some(ErrorKind::WorktreeNotFound));
    assert_eq!(error.code, -32020);
    assert!(error.message.contains("«feat-x» (ветка feat/x,"), "{}", error.message);
    assert!(error.message.contains("wt=\".\""), "{}", error.message);
    let listed = &error.data.as_ref().unwrap()["worktrees"][0];
    assert_eq!(listed["name"], "feat-x");
    assert_eq!(listed["branch"], "feat/x");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn project_without_worktrees_says_so_and_names_main_checkout() {
    let home = TempDir::new().unwrap();
    let projects = TempDir::new().unwrap();
    let plain = projects.path().join("plain");
    fs::create_dir_all(plain.join("docs")).unwrap();
    fs::write(plain.join("docs/a.md"), "# A\n").unwrap();
    let handler = handler(&home);
    handler.worker().register(&plain).unwrap();

    let error = handler
        .resolve_view(&target_with(&plain, None, Some("x")))
        .unwrap_err();
    assert_eq!(error.kind(), Some(ErrorKind::WorktreeNotFound));
    assert!(error.message.contains("нет worktree; основной checkout — wt=\".\""));
    assert_eq!(error.data.as_ref().unwrap()["worktrees"], serde_json::json!([]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_project_other_than_owner_gives_no_view_and_same_owner_keeps_view() {
    let home = TempDir::new().unwrap();
    let projects = TempDir::new().unwrap();
    let (main, work) = repo_with_worktree(projects.path());
    let other = projects.path().join("other");
    fs::create_dir_all(other.join("docs")).unwrap();
    fs::write(other.join("docs/o.md"), "# O\n").unwrap();
    let handler = handler(&home);
    handler.worker().register(&main).unwrap();
    handler.worker().register(&other).unwrap();
    let cwd = work.join("docs");

    let (project, view) = handler
        .resolve_view(&target_with(&cwd, Some(&other), None))
        .unwrap();
    assert_eq!(project, other);
    assert_eq!(view, ViewSelector::Main, "чужой проект неявного вида не даёт");
    let (project, view) = handler
        .resolve_view(&target_with(&cwd, Some(&main), None))
        .unwrap();
    assert_eq!(project, main);
    assert!(
        matches!(view, ViewSelector::Worktree(_)),
        "свой проект по имени сохраняет вид"
    );
    // Основной берётся через `wt="."`.
    let (_, view) = handler
        .resolve_view(&target_with(&cwd, Some(&main), Some(".")))
        .unwrap();
    assert_eq!(view, ViewSelector::Main);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn status_lists_worktrees_without_creating_a_delta() {
    let home = TempDir::new().unwrap();
    let projects = TempDir::new().unwrap();
    let (main, work) = repo_with_worktree(projects.path());
    fs::write(main.join(".git/worktrees/feat-x/locked"), "на флешке\n").unwrap();
    let handler = handler(&home);
    handler.worker().register(&main).unwrap();

    let status = handler
        .status(StatusParams {
            target: target_with(&work.join("docs"), None, None),
        })
        .unwrap();
    assert_eq!(status.worktrees.len(), 1);
    let item = &status.worktrees[0];
    assert_eq!(item.name, "feat-x");
    assert_eq!(item.branch.as_deref(), Some("feat/x"));
    assert_eq!(item.locked.as_deref(), Some("на флешке"));
    assert_eq!(item.delta.state, bars_protocol::DeltaState::None);
    assert!(item.current, "cwd сессии — этот worktree");
    assert!(!main.join(".bars").join("worktrees").exists(), "дельты нет");

    // Из основного checkout список тот же, но `current` нет.
    let status = handler
        .status(StatusParams {
            target: target_with(&main, None, None),
        })
        .unwrap();
    assert_eq!(status.worktrees.len(), 1);
    assert!(!status.worktrees[0].current);
    // С `wt` — только этот worktree.
    let status = handler
        .status(StatusParams {
            target: target_with(&main, None, Some("feat-x")),
        })
        .unwrap();
    assert_eq!(status.worktrees.len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn projects_filter_finds_the_owner_by_worktree_name_path_and_branch() {
    let home = TempDir::new().unwrap();
    let projects = TempDir::new().unwrap();
    let (main, _) = repo_with_worktree(projects.path());
    let handler = handler(&home);
    handler.worker().register(&main).unwrap();

    for text in ["feat-x", "FEAT/X", "runs"] {
        let found = handler
            .projects(bars_protocol::ProjectsParams {
                target: None,
                filter: Some(text.to_string()),
            })
            .await
            .unwrap();
        assert_eq!(found.projects.len(), 1, "{text}");
        let owner = &found.projects[0];
        assert_eq!(owner.path, main.display().to_string());
        assert_eq!(owner.matched_worktrees.len(), 1, "{text}");
        assert_eq!(owner.matched_worktrees[0].name, "feat-x");
    }
    let none = handler
        .projects(bars_protocol::ProjectsParams {
            target: None,
            filter: Some("нет-такого".into()),
        })
        .await
        .unwrap();
    assert!(none.projects.is_empty());
}
