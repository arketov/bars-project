//! Сквозная проверка перемещения на временном проекте: реальный диск и CAS-запись.
use crate::tree_io::{move_with_links, plan_move};
use std::fs;

fn fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("docs/a")).unwrap();
    fs::create_dir_all(temp.path().join("docs/b")).unwrap();
    fs::write(
        temp.path().join("docs/a/item.md"),
        "# Item\n[наружу](../outside.md)\n",
    )
    .unwrap();
    fs::write(temp.path().join("docs/outside.md"), "# Outside\n").unwrap();
    for (name, text) in [
        ("one.md", "[a](a/item.md#раздел)"),
        ("two.md", "[[item|подпись]]"),
        ("three.md", "[a][r]\n\n[r]: a/item.md 'title'\n"),
    ] {
        fs::write(temp.path().join("docs").join(name), text).unwrap();
    }
    temp
}

#[test]
fn rename_updates_three_documents_and_preserves_readonly_failure() {
    let temp = fixture();
    let root = temp.path();
    let file = root.join("docs/two.md");
    let mut permissions = fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&file, permissions).unwrap();
    let plan = plan_move(root, "docs", "docs/a/item.md", "docs/b", "new").unwrap();
    assert_eq!(plan.updates.len(), 3);
    let out =
        move_with_links(root, "docs", "docs/a/item.md", "docs/b", "new", &plan.token).unwrap();
    assert_eq!(out.updated.len(), 2);
    assert_eq!(out.failed.len(), 1);
    assert_eq!(out.failed[0].0, "docs/two.md");
    assert!(root.join("docs/b/new.md").exists());
    assert!(!root.join("docs/a/item.md").exists());
    assert_eq!(
        fs::read_to_string(root.join("docs/one.md")).unwrap(),
        "[a](b/new.md#раздел)"
    );
    assert_eq!(
        fs::read_to_string(root.join("docs/three.md")).unwrap(),
        "[a][r]\n\n[r]: b/new.md 'title'\n"
    );
    // Удаление временного проекта на Windows требует снять readonly.
    let mut permissions = fs::metadata(&file).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    // тест Windows: снимает атрибут «только чтение»
    permissions.set_readonly(false);
    fs::set_permissions(&file, permissions).unwrap();
}

#[test]
fn stale_preview_refuses_before_any_rename() {
    let temp = fixture();
    let root = temp.path();
    let plan = plan_move(root, "docs", "docs/a/item.md", "docs/b", "new").unwrap();
    fs::write(root.join("docs/one.md"), "чужая правка").unwrap();
    assert!(move_with_links(root, "docs", "docs/a/item.md", "docs/b", "new", &plan.token).is_err());
    assert!(root.join("docs/a/item.md").exists());
    assert!(!root.join("docs/b/new.md").exists());
    assert_eq!(
        fs::read_to_string(root.join("docs/one.md")).unwrap(),
        "чужая правка"
    );
}

#[cfg(windows)]
#[test]
fn occupied_referrer_does_not_block_other_updates_or_roll_back() {
    use std::os::windows::fs::OpenOptionsExt;
    let temp = fixture();
    let root = temp.path();
    // Разрешено чтение плана, запрещена замена файла: настоящий sharing violation Windows.
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(root.join("docs/two.md"))
        .unwrap();
    let plan = plan_move(root, "docs", "docs/a/item.md", "docs/b", "new").unwrap();
    let result =
        move_with_links(root, "docs", "docs/a/item.md", "docs/b", "new", &plan.token).unwrap();
    assert_eq!(result.updated.len(), 2);
    assert_eq!(result.failed.len(), 1);
    assert_eq!(result.failed[0].0, "docs/two.md");
    assert!(root.join("docs/b/new.md").exists());
    assert_eq!(
        fs::read_to_string(root.join("docs/two.md")).unwrap(),
        "[[item|подпись]]"
    );
    drop(held);
}

#[test]
fn folder_move_updates_outbound_and_inbound_preserves_bom_crlf() {
    let temp = fixture();
    let root = temp.path();
    fs::write(
        root.join("docs/a/item.md"),
        "\u{feff}# Item\r\n[out](../outside.md)\r\n",
    )
    .unwrap();
    fs::create_dir(root.join("docs/b/c")).unwrap();
    let plan = plan_move(root, "docs", "docs/a", "docs/b/c", "renamed").unwrap();
    let out = move_with_links(root, "docs", "docs/a", "docs/b/c", "renamed", &plan.token).unwrap();
    assert!(out.failed.is_empty());
    assert_eq!(
        fs::read_to_string(root.join("docs/b/c/renamed/item.md")).unwrap(),
        "\u{feff}# Item\r\n[out](../../../outside.md)\r\n"
    );
}

#[test]
fn appearance_follows_folder_and_document_paths() {
    let temp = fixture();
    let root = temp.path();
    crate::appearance::set(
        root,
        crate::appearance::Kind::Document,
        "a/item.md",
        Some("red"),
        None,
    )
    .unwrap();
    crate::appearance::set(
        root,
        crate::appearance::Kind::Folder,
        "a",
        Some("blue"),
        Some("book"),
    )
    .unwrap();
    crate::appearance::move_keys(root, "docs", "docs/a", "docs/b/moved").unwrap();
    let value = crate::appearance::read(root).unwrap();
    assert_eq!(
        value.documents["b/moved/item.md"].color.as_deref(),
        Some("red")
    );
    assert_eq!(value.folders["b/moved"].icon.as_deref(), Some("book"));
    assert!(!value.folders.contains_key("a"));
}
