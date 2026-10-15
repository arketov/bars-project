use std::fs;

use super::*;

/// Проект с `docs/a/x.md`, `docs/Index.md`, `README.md` и `notes.txt` в корне.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("docs/a")).unwrap();
    for file in ["docs/a/x.md", "docs/Index.md", "README.md", "notes.txt"] {
        fs::write(dir.path().join(file), "# x\n").unwrap();
    }
    dir
}

fn resolve(dir: &tempfile::TempDir, path: &str) -> Result<PathBuf, DocPathError> {
    resolve_doc_path(dir.path(), Path::new("docs"), path)
}

#[test]
fn relative_path_goes_from_the_docs_root() {
    let dir = project();
    assert_eq!(
        resolve(&dir, "a/x.md").unwrap(),
        dir.path().join("docs").join("a/x.md")
    );
}

#[test]
fn explicit_dot_slash_goes_from_the_project_root() {
    let dir = project();
    assert_eq!(
        resolve(&dir, "./README.md").unwrap(),
        dir.path().join("README.md")
    );
    // `./Index.md` — это корень проекта, а не корень документации.
    assert_eq!(
        resolve(&dir, "./Index.md"),
        Err(DocPathError::NotFound("./Index.md".into()))
    );
}

#[test]
fn path_from_search_results_falls_back_to_the_project_root() {
    let dir = project();
    assert_eq!(
        resolve(&dir, "docs/a/x.md").unwrap(),
        dir.path().join("docs/a/x.md")
    );
    assert_eq!(
        resolve(&dir, "README.md").unwrap(),
        dir.path().join("README.md")
    );
}

#[test]
fn docs_root_wins_over_the_project_root() {
    let dir = project();
    fs::write(dir.path().join("Index.md"), "# корень проекта\n").unwrap();
    assert_eq!(
        resolve(&dir, "Index.md").unwrap(),
        dir.path().join("docs").join("Index.md")
    );
}

#[test]
fn absolute_path_is_taken_as_is() {
    let dir = project();
    let other = tempfile::tempdir().unwrap();
    let file = other.path().join("outside.md");
    fs::write(&file, "# вне\n").unwrap();
    assert_eq!(resolve(&dir, file.to_str().unwrap()).unwrap(), file);
}

#[test]
fn backslashes_work_like_slashes() {
    let dir = project();
    assert_eq!(
        resolve(&dir, "a\\x.md").unwrap(),
        dir.path().join("docs").join("a/x.md")
    );
    assert_eq!(
        resolve(&dir, ".\\README.md").unwrap(),
        dir.path().join("README.md")
    );
}

#[test]
fn only_markdown_files_are_allowed() {
    let dir = project();
    for path in ["notes.txt", "./notes.txt", "a", "", "md"] {
        assert_eq!(
            resolve(&dir, path),
            Err(DocPathError::NotMarkdown(path.into())),
            "{path:?}"
        );
    }
}

#[test]
fn uppercase_extension_is_markdown() {
    let dir = project();
    fs::write(dir.path().join("docs/Big.MD"), "# x\n").unwrap();
    assert!(resolve(&dir, "Big.MD").is_ok());
}

#[test]
fn missing_file_and_directory_are_not_found() {
    let dir = project();
    fs::create_dir(dir.path().join("docs/folder.md")).unwrap();
    for path in ["missing.md", "a/missing.md", "folder.md"] {
        assert_eq!(
            resolve(&dir, path),
            Err(DocPathError::NotFound(path.into())),
            "{path:?}"
        );
    }
}

#[test]
fn docs_root_dot_is_the_project_root() {
    let dir = project();
    assert_eq!(
        resolve_doc_path(dir.path(), Path::new("."), "README.md").unwrap(),
        dir.path().join(".").join("README.md")
    );
}
