//! Проверки дерева по диску и операций с файлами и папками (T-01…T-08 из tree-and-files.md).

use std::fs;
use std::path::Path;

use crate::tree_io::{
    TreeError, check_name, create_dir, create_doc, move_entry, plan_delete, read_tree, trash_entry,
};

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("docs/a")).unwrap();
    fs::write(dir.path().join("docs/a/x.md"), "# x").unwrap();
    dir
}

fn link_dir(target: &Path, link: &Path) -> bool {
    #[cfg(windows)]
    {
        let back = |p: &Path| p.to_string_lossy().replace('/', "\\");
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(back(link))
            .arg(back(target))
            .output()
            .is_ok_and(|out| out.status.success())
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
}

#[test]
fn t01_empty_folder_and_new_file_are_visible_at_once() {
    let dir = project();
    fs::create_dir_all(dir.path().join("docs/empty")).unwrap();
    let before = read_tree(dir.path(), "docs").unwrap();
    assert_eq!(before.dirs, ["docs/a", "docs/empty"]);

    let path = create_doc(dir.path(), "docs", "docs/empty", "новый").unwrap();
    assert_eq!(path, "docs/empty/новый.md");
    let after = read_tree(dir.path(), "docs").unwrap();
    assert!(after.files.iter().any(|f| f.path == path));
    assert_ne!(before.revision, after.revision);
    assert_eq!(
        after.revision,
        read_tree(dir.path(), "docs").unwrap().revision
    );
}

#[test]
#[ignore = "явный замер обхода 10 000 документов"]
fn measure_tree_10000_documents() {
    let dir = tempfile::tempdir().unwrap();
    for module in 0..100 {
        let folder = dir.path().join(format!("docs/m{module:03}"));
        fs::create_dir_all(&folder).unwrap();
        for file in 0..100 {
            fs::write(folder.join(format!("d{file:03}.md")), "# t").unwrap();
        }
    }
    let mut times = Vec::new();
    for _ in 0..5 {
        let start = std::time::Instant::now();
        let tree = read_tree(dir.path(), "docs").unwrap();
        times.push(start.elapsed().as_millis());
        assert_eq!(tree.files.len(), 10000);
        assert_eq!(tree.dirs.len(), 100);
    }
    println!("tree_read 10000 документов, мс: {times:?}");
}

#[test]
fn t02_tree_matches_indexer_walk() {
    let dir = project();
    fs::write(dir.path().join("docs/BARS_INSTRUCTION.md"), "# i").unwrap();
    fs::create_dir_all(dir.path().join("docs/.hidden")).unwrap();
    fs::write(dir.path().join("docs/.hidden/h.md"), "# h").unwrap();
    fs::write(dir.path().join("docs/note.txt"), "t").unwrap();
    fs::write(dir.path().join("docs/UP.MD"), "# u").unwrap();
    fs::create_dir_all(dir.path().join("outside")).unwrap();
    fs::write(dir.path().join("outside/s.md"), "# s").unwrap();
    link_dir(&dir.path().join("outside"), &dir.path().join("docs/link"));

    let tree = read_tree(dir.path(), "docs").unwrap();
    let walk = bars_core::indexing::walk::walk_docs(dir.path(), Path::new("docs")).unwrap();
    let paths: Vec<_> = tree.files.iter().map(|f| f.path.as_str()).collect();
    let walked: Vec<_> = walk.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, walked);
    assert_eq!(paths, ["docs/UP.MD", "docs/a/x.md"]);
    assert_eq!(tree.dirs, ["docs/a"]);
}

#[test]
fn t03_names_and_paths_are_checked() {
    let dir = project();
    let bad_names = [
        "",
        "a/b",
        "a\\b",
        "a:b",
        "a*",
        "a?",
        "a\"",
        "a<",
        "a>",
        "a|",
        "a ",
        "a.",
        " a",
        "CON",
        "nul.txt",
        "com1",
        "tab\tname",
        ".md",
    ];
    for name in bad_names {
        assert!(
            matches!(check_name(name, false), Err(TreeError::InvalidName(_))),
            "документ {name:?}"
        );
    }
    assert!(check_name(&"я".repeat(201), false).is_err());
    assert!(matches!(
        check_name(".git", true),
        Err(TreeError::InvalidName(_))
    ));
    assert_eq!(check_name("idea", false).unwrap(), "idea.md");
    assert_eq!(check_name("IDEA.MD", false).unwrap(), "IDEA.MD");
    assert_eq!(
        check_name("Папка с пробелом", true).unwrap(),
        "Папка с пробелом"
    );

    // Пути: `..`, абсолютный, вне корня документации, несуществующая папка.
    for parent in ["docs/../x", "../docs", "other", "docs/missing"] {
        assert!(
            create_doc(dir.path(), "docs", parent, "n").is_err(),
            "{parent}"
        );
    }
    let abs = dir.path().join("docs").to_string_lossy().into_owned();
    assert!(create_doc(dir.path(), "docs", &abs, "n").is_err());
    assert!(matches!(
        create_doc(dir.path(), "docs", "docs", "BARS_INSTRUCTION"),
        Err(TreeError::InvalidName(_))
    ));
    // Junction наружу.
    fs::create_dir_all(dir.path().join("outside")).unwrap();
    if link_dir(&dir.path().join("outside"), &dir.path().join("docs/link")) {
        assert_eq!(
            create_doc(dir.path(), "docs", "docs/link", "n"),
            Err(TreeError::OutsideRoot)
        );
        assert!(!dir.path().join("outside/n.md").exists());
    }
    // Служебные узлы не переносятся и не удаляются.
    fs::write(dir.path().join("docs/BARS_INSTRUCTION.md"), "# i").unwrap();
    assert!(
        move_entry(
            dir.path(),
            "docs",
            "docs/BARS_INSTRUCTION.md",
            "docs/a",
            "z"
        )
        .is_err()
    );
    assert!(trash_entry(dir.path(), "docs", "docs/BARS_INSTRUCTION.md").is_err());
    assert!(trash_entry(dir.path(), "docs", "docs").is_err());
    assert!(dir.path().join("docs/BARS_INSTRUCTION.md").exists());
}

#[test]
fn t03_case_collision_is_refused() {
    let dir = project();
    fs::write(dir.path().join("docs/a/Index.md"), "# i").unwrap();
    let error = create_doc(dir.path(), "docs", "docs/a", "index").unwrap_err();
    assert_eq!(error, TreeError::Exists("Index.md".into()));
    assert!(matches!(
        create_dir(dir.path(), "docs", "docs", "A"),
        Err(TreeError::Exists(name)) if name == "a"
    ));
}

#[test]
fn t04_new_folder_has_index_with_title_and_heading() {
    let dir = project();
    let created = create_dir(dir.path(), "docs", "docs", "Новый модуль").unwrap();
    assert_eq!(created.path, "docs/Новый модуль");
    assert_eq!(created.index_path, "docs/Новый модуль/index.md");
    assert!(created.index_error.is_none());
    let text = fs::read_to_string(dir.path().join("docs/Новый модуль/index.md")).unwrap();
    assert_eq!(text, "---\ntitle: Новый модуль\n---\n\n# Новый модуль\n");
}

#[test]
fn t04_title_with_special_characters_is_quoted() {
    let dir = project();
    let path = create_doc(dir.path(), "docs", "docs", "a #1 'q'").unwrap();
    let text = fs::read_to_string(dir.path().join(&path)).unwrap();
    assert!(text.starts_with("---\ntitle: \"a #1 'q'\"\n---"), "{text}");
}

#[test]
fn t04_repeated_folder_is_refused_and_keeps_the_first() {
    let dir = project();
    create_dir(dir.path(), "docs", "docs", "m").unwrap();
    assert!(matches!(
        create_dir(dir.path(), "docs", "docs", "m"),
        Err(TreeError::Exists(_))
    ));
    assert!(dir.path().join("docs/m/index.md").exists());
}

#[test]
fn t05_rename_keeps_content_and_does_not_touch_links() {
    let dir = project();
    fs::write(dir.path().join("docs/a/y.md"), "[x](x.md)").unwrap();
    let new = move_entry(dir.path(), "docs", "docs/a/x.md", "docs/a", "z.md").unwrap();
    assert_eq!(new, "docs/a/z.md");
    assert_eq!(
        fs::read_to_string(dir.path().join("docs/a/z.md")).unwrap(),
        "# x"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("docs/a/y.md")).unwrap(),
        "[x](x.md)"
    );
    assert!(!dir.path().join("docs/a/x.md").exists());
    // Занятая цель — отказ без изменений.
    let error = move_entry(dir.path(), "docs", "docs/a/z.md", "docs/a", "Y").unwrap_err();
    assert_eq!(error, TreeError::Exists("y.md".into()));
    assert!(dir.path().join("docs/a/z.md").exists());
}

#[test]
fn t06_folder_move_carries_its_documents() {
    let dir = project();
    fs::create_dir_all(dir.path().join("docs/b")).unwrap();
    let new = move_entry(dir.path(), "docs", "docs/a", "docs/b", "a").unwrap();
    assert_eq!(new, "docs/b/a");
    assert!(dir.path().join("docs/b/a/x.md").exists());
    // Папку нельзя поместить внутрь неё самой.
    let error = move_entry(dir.path(), "docs", "docs/b", "docs/b/a", "b").unwrap_err();
    assert!(matches!(error, TreeError::InvalidMove(_)));
    assert!(dir.path().join("docs/b/a/x.md").exists());
    // Тот же путь — не перемещение.
    assert!(matches!(
        move_entry(dir.path(), "docs", "docs/b/a", "docs/b", "a"),
        Err(TreeError::InvalidMove(_))
    ));
}

#[test]
fn t07_case_only_rename() {
    let dir = project();
    let new = move_entry(dir.path(), "docs", "docs/a/x.md", "docs/a", "X.md").unwrap();
    assert_eq!(new, "docs/a/X.md");
    let names: Vec<_> = fs::read_dir(dir.path().join("docs/a"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["X.md"]);
}

#[test]
fn rename_never_overwrites_an_existing_staging_path() {
    let dir = project();
    let staging = dir
        .path()
        .join(format!("docs/a/.bars-rename-{}", std::process::id()));
    fs::write(&staging, "чужие данные").unwrap();
    assert!(matches!(
        move_entry(dir.path(), "docs", "docs/a/x.md", "docs/a", "X.md"),
        Err(TreeError::Exists(_))
    ));
    assert_eq!(fs::read_to_string(&staging).unwrap(), "чужие данные");
    assert_eq!(
        fs::read_to_string(dir.path().join("docs/a/x.md")).unwrap(),
        "# x"
    );
}

#[cfg(windows)]
#[test]
fn late_destination_is_not_overwritten_by_the_os_move() {
    let dir = project();
    let source = dir.path().join("docs/a/x.md");
    let destination = dir.path().join("docs/a/late.md");
    fs::write(&destination, "появился после проверки").unwrap();
    assert!(crate::tree_io::rename_without_replace(&source, &destination).is_err());
    assert_eq!(fs::read_to_string(source).unwrap(), "# x");
    assert_eq!(
        fs::read_to_string(destination).unwrap(),
        "появился после проверки"
    );
}

#[test]
fn hidden_ancestors_cannot_be_changed_through_tree_commands() {
    let dir = project();
    fs::create_dir_all(dir.path().join("docs/.hidden/inner")).unwrap();
    fs::write(dir.path().join("docs/.hidden/inner/a.md"), "# a").unwrap();
    assert!(create_doc(dir.path(), "docs", "docs/.hidden/inner", "b").is_err());
    assert!(move_entry(dir.path(), "docs", "docs/.hidden/inner/a.md", "docs/a", "b").is_err());
    assert!(trash_entry(dir.path(), "docs", "docs/.hidden/inner/a.md").is_err());
}

#[test]
fn t08_trash_removes_the_node_or_refuses_without_deleting() {
    let dir = project();
    fs::create_dir_all(dir.path().join("docs/gone")).unwrap();
    fs::write(dir.path().join("docs/gone/g.md"), "# g").unwrap();
    match trash_entry(dir.path(), "docs", "docs/gone") {
        Ok(()) => {
            assert!(!dir.path().join("docs/gone").exists());
            #[cfg(windows)]
            {
                let original = dir.path().join("docs/gone").canonicalize();
                assert!(original.is_err());
                let parent = dir.path().join("docs").canonicalize().unwrap();
                let items: Vec<_> = trash::os_limited::list()
                    .unwrap()
                    .into_iter()
                    .filter(|item| {
                        item.name == "gone"
                            && item.original_parent.canonicalize().ok().as_ref() == Some(&parent)
                    })
                    .collect();
                assert_eq!(items.len(), 1, "узел действительно в корзине");
                trash::os_limited::restore_all(items).unwrap();
                assert_eq!(
                    fs::read_to_string(dir.path().join("docs/gone/g.md")).unwrap(),
                    "# g"
                );
            }
        }
        // Среда без корзины: отказ и ничего не удалено.
        Err(TreeError::TrashUnavailable(_)) => assert!(dir.path().join("docs/gone/g.md").exists()),
        Err(other) => panic!("{other:?}"),
    }
    assert_eq!(
        trash_entry(dir.path(), "docs", "docs/nothing.md"),
        Err(TreeError::NotFound)
    );
}

#[test]
fn t08_delete_plan_lists_the_documents_and_changes_nothing() {
    let dir = project();
    fs::write(dir.path().join("docs/a/y.md"), "# y").unwrap();
    fs::write(dir.path().join("docs/a/skip.txt"), "не документ").unwrap();
    fs::write(dir.path().join("docs/ab.md"), "# ab").unwrap();
    let folder = plan_delete(dir.path(), "docs", "docs/a").unwrap();
    assert!(folder.folder);
    assert_eq!(folder.documents, vec!["docs/a/x.md", "docs/a/y.md"]);
    let file = plan_delete(dir.path(), "docs", "docs/ab.md").unwrap();
    assert!(!file.folder);
    assert_eq!(file.documents, vec!["docs/ab.md"]);
    assert_eq!(
        plan_delete(dir.path(), "docs", "docs/nothing.md").err(),
        Some(TreeError::NotFound)
    );
    assert_eq!(
        plan_delete(dir.path(), "docs", "docs/a/skip.txt").err(),
        Some(TreeError::InvalidPath)
    );
    assert!(dir.path().join("docs/a/x.md").exists());
}

#[test]
fn nested_and_dot_docs_root() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("m/docs")).unwrap();
    let path = create_doc(dir.path(), "m/docs", "m/docs", "n").unwrap();
    assert_eq!(path, "m/docs/n.md");

    let flat = tempfile::tempdir().unwrap();
    let path = create_doc(flat.path(), ".", ".", "n").unwrap();
    assert_eq!(path, "n.md");
    let folder = create_dir(flat.path(), ".", ".", "mod").unwrap();
    assert_eq!(folder.index_path, "mod/index.md");
    assert_eq!(
        move_entry(flat.path(), ".", "n.md", "mod", "n.md").unwrap(),
        "mod/n.md"
    );
}
