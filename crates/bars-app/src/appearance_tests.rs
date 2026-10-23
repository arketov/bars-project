//! Проверки Т-38: чтение и запись `.bars/appearance.toml`, допустимые пути и значения.

use std::fs;

use crate::appearance::*;

fn project() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

#[test]
fn missing_file_reads_as_empty() {
    let dir = project();
    assert_eq!(read(dir.path()).unwrap(), Appearance::default());
}

#[test]
fn set_writes_and_read_returns_the_same_entries() {
    let dir = project();
    set(
        dir.path(),
        Kind::Folder,
        "features",
        Some("blue"),
        Some("book"),
    )
    .unwrap();
    set(
        dir.path(),
        Kind::Folder,
        "features/knowledge-base",
        Some("teal"),
        None,
    )
    .unwrap();
    set(
        dir.path(),
        Kind::Document,
        "features/window/main-window.md",
        Some("pink"),
        None,
    )
    .unwrap();
    let got = read(dir.path()).unwrap();
    assert_eq!(
        got.folders["features"],
        Entry {
            color: Some("blue".into()),
            icon: Some("book".into())
        }
    );
    assert_eq!(got.folders["features/knowledge-base"].icon, None);
    assert_eq!(
        got.documents["features/window/main-window.md"]
            .color
            .as_deref(),
        Some("pink")
    );
    let text = fs::read_to_string(dir.path().join(".bars/appearance.toml")).unwrap();
    assert!(
        text.contains("\"features/knowledge-base\" = { color = \"teal\" }"),
        "{text}"
    );
    assert!(!dir.path().join(".bars/appearance.toml.bars-tmp").exists());
}

#[test]
fn resetting_every_field_removes_the_entry_and_the_empty_section() {
    let dir = project();
    set(
        dir.path(),
        Kind::Folder,
        "manual",
        Some("green"),
        Some("book"),
    )
    .unwrap();
    set(dir.path(), Kind::Folder, "manual", None, Some("book")).unwrap();
    assert_eq!(read(dir.path()).unwrap().folders["manual"].color, None);
    set(dir.path(), Kind::Folder, "manual", None, None).unwrap();
    assert!(read(dir.path()).unwrap().folders.is_empty());
    assert!(
        !fs::read_to_string(dir.path().join(".bars/appearance.toml"))
            .unwrap()
            .contains("folders")
    );
}

#[test]
fn folders_deeper_than_second_level_are_rejected() {
    let dir = project();
    assert!(set(dir.path(), Kind::Folder, "a/b", Some("red"), Some("star")).is_ok());
    let error = set(dir.path(), Kind::Folder, "a/b/c", Some("red"), None).unwrap_err();
    assert_eq!(error.kind(), "not_allowed");
}

#[test]
fn document_takes_a_color_only() {
    let dir = project();
    assert_eq!(
        set(
            dir.path(),
            Kind::Document,
            "a.md",
            Some("red"),
            Some("star")
        )
        .unwrap_err()
        .kind(),
        "not_allowed"
    );
    assert_eq!(
        set(dir.path(), Kind::Document, "a.txt", Some("red"), None)
            .unwrap_err()
            .kind(),
        "not_allowed"
    );
}

#[test]
fn bad_paths_and_values_are_rejected_without_touching_the_file() {
    let dir = project();
    for path in ["", "../x", "a/../b", "a//b", "a\\b", "./a", "/a"] {
        assert_eq!(
            set(dir.path(), Kind::Folder, path, Some("red"), None)
                .unwrap_err()
                .kind(),
            "not_allowed",
            "{path}"
        );
    }
    assert_eq!(
        set(dir.path(), Kind::Folder, "a", Some("#ff0000"), None)
            .unwrap_err()
            .kind(),
        "invalid_value"
    );
    assert_eq!(
        set(dir.path(), Kind::Folder, "a", None, Some("пони"))
            .unwrap_err()
            .kind(),
        "invalid_value"
    );
    assert!(!dir.path().join(".bars/appearance.toml").exists());
}

#[test]
fn broken_file_is_reported_and_never_overwritten() {
    let dir = project();
    fs::create_dir_all(dir.path().join(".bars")).unwrap();
    let file = dir.path().join(".bars/appearance.toml");
    fs::write(&file, "[folders\ncolor = ").unwrap();
    assert_eq!(read(dir.path()).unwrap_err().kind(), "broken");
    assert_eq!(
        set(dir.path(), Kind::Folder, "a", Some("red"), None)
            .unwrap_err()
            .kind(),
        "broken"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "[folders\ncolor = ");
}

#[test]
fn wrong_types_inside_a_valid_file_count_as_broken() {
    assert_eq!(
        parse("[folders.a]\ncolor = 5\n").unwrap_err().kind(),
        "broken"
    );
    assert_eq!(parse("folders = 3\n").unwrap_err().kind(), "broken");
}

#[test]
fn unknown_values_are_kept_for_the_window_to_ignore() {
    let got = parse("[folders.a]\ncolor = \"mauve\"\nicon = \"ufo\"\n").unwrap();
    assert_eq!(got.folders["a"].color.as_deref(), Some("mauve"));
}

#[test]
fn palette_and_icons_have_no_duplicates() {
    let mut colors = COLORS.to_vec();
    colors.sort_unstable();
    colors.dedup();
    assert_eq!(colors.len(), COLORS.len());
    let mut icons = ICONS.to_vec();
    icons.sort_unstable();
    icons.dedup();
    assert_eq!(icons.len(), ICONS.len());
}

#[test]
fn module_index_survives_copy_and_preserves_content() {
    let first = project();
    let second = project();
    for dir in [&first, &second] {
        fs::create_dir_all(dir.path().join("notes/features")).unwrap();
    }
    let file = first.path().join("notes/features/index.md");
    fs::write(
        &file,
        "\u{feff}---\r\ntitle: Фичи\r\ncustom: keep\r\n---\r\n\r\n# Фичи\r\nТекст\r\n",
    )
    .unwrap();
    let got = set_for_docs(
        first.path(),
        "notes",
        Kind::Folder,
        "features",
        Some("teal"),
        Some("gear"),
    )
    .unwrap();
    assert_eq!(got.folders["features"].icon.as_deref(), Some("gear"));
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.starts_with("\u{feff}---\r\ntitle: Фичи\r\ncustom: keep\r\n"));
    assert!(text.ends_with("---\r\n\r\n# Фичи\r\nТекст\r\n"));
    assert!(!first.path().join(".bars/appearance.toml").exists());
    fs::copy(file, second.path().join("notes/features/Index.md")).unwrap();
    assert_eq!(read_for_docs(second.path(), "notes").unwrap(), got);
    set_for_docs(second.path(), "notes", Kind::Folder, "features", None, None).unwrap();
    assert!(
        !read_for_docs(second.path(), "notes")
            .unwrap()
            .folders
            .contains_key("features")
    );
    assert_eq!(read_for_docs(first.path(), "notes").unwrap(), got);
}

#[test]
fn module_index_creation_and_broken_frontmatter() {
    let dir = project();
    fs::create_dir_all(dir.path().join("docs/module")).unwrap();
    set_for_docs(
        dir.path(),
        "docs",
        Kind::Folder,
        "module",
        Some("blue"),
        None,
    )
    .unwrap();
    let file = dir.path().join("docs/module/Index.md");
    assert!(
        fs::read_to_string(&file)
            .unwrap()
            .contains("title: \"module\"")
    );
    for bad in [
        "---\ntitle: broken\n",
        "---\ncolor: [blue]\n---\n",
        "---\nicon: gear\nicon: book\n---\n",
    ] {
        fs::write(&file, bad).unwrap();
        assert!(
            set_for_docs(
                dir.path(),
                "docs",
                Kind::Folder,
                "module",
                Some("red"),
                None
            )
            .is_err()
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), bad);
    }
    assert!(set_for_docs(dir.path(), "docs", Kind::Folder, "..", Some("red"), None).is_err());
}

#[test]
fn module_index_overrides_legacy_and_local_entries_stay_local() {
    let dir = project();
    fs::create_dir_all(dir.path().join("docs/module")).unwrap();
    set(
        dir.path(),
        Kind::Folder,
        "module",
        Some("red"),
        Some("book"),
    )
    .unwrap();
    set(dir.path(), Kind::Folder, "module/sub", Some("pink"), None).unwrap();
    let migrated = read_for_docs(dir.path(), "docs").unwrap();
    assert_eq!(migrated.folders["module"].color.as_deref(), Some("red"));
    assert!(!read(dir.path()).unwrap().folders.contains_key("module"));
    set_for_docs(
        dir.path(),
        "docs",
        Kind::Folder,
        "module",
        Some("blue"),
        Some("gear"),
    )
    .unwrap();
    let got = read_for_docs(dir.path(), "docs").unwrap();
    assert_eq!(got.folders["module"].color.as_deref(), Some("blue"));
    assert_eq!(got.folders["module/sub"].color.as_deref(), Some("pink"));
    set_for_docs(dir.path(), "docs", Kind::Folder, "module", None, None).unwrap();
    assert!(
        !read_for_docs(dir.path(), "docs")
            .unwrap()
            .folders
            .contains_key("module")
    );
}

#[test]
fn migration_keeps_explicit_index_and_module_move_carries_style() {
    let dir = project();
    fs::create_dir_all(dir.path().join("docs/old")).unwrap();
    fs::write(
        dir.path().join("docs/old/Index.md"),
        "---\ntitle: Old\ncolor: teal\nicon: gear\n---\n# Old\n",
    )
    .unwrap();
    set(dir.path(), Kind::Folder, "old", Some("red"), Some("book")).unwrap();
    let got = read_for_docs(dir.path(), "docs").unwrap();
    assert_eq!(got.folders["old"].color.as_deref(), Some("teal"));
    fs::rename(dir.path().join("docs/old"), dir.path().join("docs/new")).unwrap();
    let got = read_for_docs(dir.path(), "docs").unwrap();
    assert!(!got.folders.contains_key("old"));
    assert_eq!(got.folders["new"].icon.as_deref(), Some("gear"));
}
