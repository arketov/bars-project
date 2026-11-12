//! Окно и worktree (шаг 6.3.6): допуск путей от корня вида, запись документа в файл W и перенос без
//! обновления ссылок. Диск настоящий, воркер не нужен: путь команд окна — `resolve` и `tree_io` от корня вида.
use crate::document_io::{Eol, WriteOutcome, read, resolve, write};
use crate::tree_io::{move_without_links, plan_move_without_links};
use std::fs;

/// Владелец и рабочая копия с одним и тем же `docs/a/item.md` и ссылкой на него из `docs/one.md`.
fn owner_and_view() -> (tempfile::TempDir, tempfile::TempDir) {
    let make = || {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("docs/a")).unwrap();
        fs::create_dir_all(temp.path().join("docs/b")).unwrap();
        fs::write(temp.path().join("docs/a/item.md"), "# Item\n").unwrap();
        fs::write(temp.path().join("docs/one.md"), "[a](a/item.md)\n").unwrap();
        temp
    };
    (make(), make())
}

#[test]
fn editor_write_in_view_goes_to_worktree_file() {
    let (owner, view) = owner_and_view();
    let file = resolve(view.path(), "docs", "docs/a/item.md").unwrap();
    assert!(file.starts_with(view.path().canonicalize().unwrap()));
    let revision = read(&file).unwrap().revision;
    let outcome = write(
        &file,
        "# Правка в worktree\n",
        Some(&revision),
        Eol::Lf,
        false,
    )
    .unwrap();
    assert!(matches!(outcome, WriteOutcome::Saved { .. }));
    assert_eq!(
        fs::read_to_string(view.path().join("docs/a/item.md")).unwrap(),
        "# Правка в worktree\n"
    );
    assert_eq!(
        fs::read_to_string(owner.path().join("docs/a/item.md")).unwrap(),
        "# Item\n"
    );
}

#[test]
fn path_admission_is_counted_from_the_view_root() {
    let (owner, view) = owner_and_view();
    for escape in ["../x.md", "docs/../../x.md"] {
        assert!(resolve(view.path(), "docs", escape).is_err(), "{escape}");
    }
    // Абсолютный путь к документу владельца вид не принимает.
    let foreign = owner.path().join("docs/a/item.md");
    assert!(resolve(view.path(), "docs", &foreign.display().to_string()).is_err());
}

#[test]
fn rename_in_view_moves_the_file_and_leaves_links_alone() {
    let (_owner, view) = owner_and_view();
    let plan =
        plan_move_without_links(view.path(), "docs", "docs/a/item.md", "docs/b", "new").unwrap();
    assert!(plan.updates.is_empty() && plan.warnings.is_empty());
    let result = move_without_links(
        view.path(),
        "docs",
        "docs/a/item.md",
        "docs/b",
        "new",
        &plan.token,
    )
    .unwrap();
    assert_eq!(result.path, "docs/b/new.md");
    assert!(result.updated.is_empty());
    assert!(view.path().join("docs/b/new.md").is_file());
    assert!(!view.path().join("docs/a/item.md").exists());
    // Ссылка в соседнем документе вида не обновлена: индекса ссылок у вида нет.
    assert_eq!(
        fs::read_to_string(view.path().join("docs/one.md")).unwrap(),
        "[a](a/item.md)\n"
    );
}

#[test]
fn move_in_view_refuses_a_stale_token() {
    let (_owner, view) = owner_and_view();
    let plan =
        plan_move_without_links(view.path(), "docs", "docs/a/item.md", "docs/b", "new").unwrap();
    assert!(
        move_without_links(
            view.path(),
            "docs",
            "docs/a/item.md",
            "docs/b",
            "other",
            &plan.token
        )
        .is_err()
    );
    assert!(view.path().join("docs/a/item.md").is_file());
}
