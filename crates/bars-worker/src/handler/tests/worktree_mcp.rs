//! `wt` у вызовов MCP и CLI на стороне воркера: сверка вида через `reindex`, отказ полной
//! переиндексации, `session_info` с worktree, `bars_status` с `wt` (часть WORKTREES). Тесты не
//! запускались: сборка запрещена в этом шаге.

use super::worktree_search::{handler, repo_with_worktree, target_wt, wait_delta, wait_idle};
use super::*;
use bars_protocol::{DeltaState, ReindexParams, SessionInfoParams, StatusParams};

fn reindex_params(target: Target, full: bool) -> ReindexParams {
    ReindexParams {
        target,
        full,
        accept_model_change: false,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reindex_with_wt_resyncs_the_delta_and_full_is_rejected() {
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

    // `bars_status refresh=true wt=…` и `bars-cli reindex --wt`: первое обращение строит дельту.
    let accepted = handler
        .reindex(reindex_params(target_wt(&main, Some("feat-x")), false))
        .unwrap();
    assert!(accepted.accepted);
    wait_delta(worker, &main).await;

    // Повторная сверка на готовой дельте тоже принимается.
    assert!(
        handler
            .reindex(reindex_params(target_wt(&main, Some("feat-x")), false))
            .unwrap()
            .accepted
    );
    wait_delta(worker, &main).await;

    // Полная переиндексация и согласие на смену модели у дельты не бывают.
    let error = handler
        .reindex(reindex_params(target_wt(&main, Some("feat-x")), true))
        .unwrap_err();
    assert_eq!(error.kind(), Some(ErrorKind::InvalidParams));
    assert_eq!(
        error.data.as_ref().and_then(|data| data["kind"].as_str()),
        Some("worktree_unsupported")
    );
    let mut consent = reindex_params(target_wt(&main, Some("feat-x")), false);
    consent.accept_model_change = true;
    assert_eq!(
        handler.reindex(consent).unwrap_err().kind(),
        Some(ErrorKind::InvalidParams)
    );

    // Неизвестный вид — `worktree_not_found`, основной checkout (`.`) — обычная проверка.
    assert_eq!(
        handler
            .reindex(reindex_params(target_wt(&main, Some("nope")), false))
            .unwrap_err()
            .kind(),
        Some(ErrorKind::WorktreeNotFound)
    );
    assert!(
        handler
            .reindex(reindex_params(target_wt(&main, Some(".")), false))
            .unwrap()
            .accepted
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn status_with_wt_lists_only_that_worktree_without_building_it() {
    let home = TempDir::new().unwrap();
    let projects = TempDir::new().unwrap();
    let (main, work) = repo_with_worktree(projects.path());
    let handler = handler(&home);
    let worker = handler.worker();
    worker.register(&main).unwrap();
    wait_idle(worker).await;

    let all = handler
        .status(StatusParams {
            target: target_wt(&main, None),
        })
        .unwrap();
    assert_eq!(all.worktrees.len(), 1);
    assert!(!all.worktrees[0].current, "сессия в основном checkout");

    let one = handler
        .status(StatusParams {
            target: target_wt(&main, Some("feat-x")),
        })
        .unwrap();
    assert_eq!(one.worktrees.len(), 1);
    assert_eq!(one.worktrees[0].name, "feat-x");
    assert_eq!(
        one.worktrees[0].delta.state,
        DeltaState::None,
        "status дельту не строит"
    );

    // Сессия в worktree: он помечен `current` и без `wt`.
    let current = handler
        .status(StatusParams {
            target: target_wt(&work, None),
        })
        .unwrap();
    assert!(current.worktrees[0].current);
    assert_eq!(
        bars_protocol::view_worktree(None, &current.worktrees).map(|item| item.name.as_str()),
        Some("feat-x")
    );
    assert!(bars_protocol::view_worktree(Some("."), &current.worktrees).is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_info_names_the_worktree_of_the_session() {
    let home = TempDir::new().unwrap();
    let projects = TempDir::new().unwrap();
    let (main, work) = repo_with_worktree(projects.path());
    let handler = handler(&home);
    let worker = handler.worker();
    worker.register(&main).unwrap();
    wait_idle(worker).await;

    let info = |cwd: &Path, wt: Option<&str>| SessionInfoParams {
        cwd: cwd.display().to_string(),
        project: None,
        wt: wt.map(str::to_string),
    };
    // Сессия в основном checkout: worktree нет.
    let project = handler
        .session_info(info(&main, None))
        .await
        .unwrap()
        .project
        .unwrap();
    assert!(project.worktree.is_none());
    // Сессия в worktree: имя проекта владельца, worktree с веткой.
    let project = handler
        .session_info(info(&work, None))
        .await
        .unwrap()
        .project
        .unwrap();
    let worktree = project.worktree.expect("сессия в worktree");
    assert_eq!(worktree.name, "feat-x");
    assert_eq!(worktree.branch.as_deref(), Some("feat/x"));
    // Явный `wt` проверяет цель: `.` — основной, неизвестный — ошибка.
    let project = handler
        .session_info(info(&work, Some(".")))
        .await
        .unwrap()
        .project
        .unwrap();
    assert!(project.worktree.is_none());
    let error = handler
        .session_info(info(&main, Some("nope")))
        .await
        .unwrap_err();
    assert_eq!(error.kind(), Some(ErrorKind::WorktreeNotFound));
}
