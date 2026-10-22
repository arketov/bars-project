//! Проверки E-01, E-04, E-05, E-07, E-08, E-10 проекта редактора: допуск пути, чтение и запись документа.

use std::fs;
use std::path::Path;

use crate::document_io::*;

/// Проект с `docs/a/x.md`, `docs/index.md`, `README.md` вне корня документации и каталогом `docs/dir.md`.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("docs/a")).unwrap();
    fs::create_dir_all(dir.path().join("docs/dir.md")).unwrap();
    fs::write(dir.path().join("docs/a/x.md"), "# x\n").unwrap();
    fs::write(dir.path().join("docs/index.md"), "# i\n").unwrap();
    fs::write(dir.path().join("README.md"), "# r\n").unwrap();
    dir
}

fn temp_files(dir: &Path) -> Vec<String> {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".bars-tmp"))
        .collect()
}

#[test]
fn resolve_accepts_existing_and_new_documents_inside_docs_root() {
    let dir = project();
    let existing = resolve(dir.path(), "docs", "docs/a/x.md").unwrap();
    assert!(existing.ends_with("docs/a/x.md") && existing.is_file());
    let new = resolve(dir.path(), "docs", "docs/a/new.MD").unwrap();
    assert!(!new.exists() && new.parent().unwrap().is_dir());
    assert!(resolve(dir.path(), "./docs", "docs\\a\\x.md").is_ok());
    assert!(resolve(dir.path(), ".", "README.md").is_ok());
}

#[test]
fn resolve_rejects_escapes_foreign_files_and_bad_targets() {
    let dir = project();
    for (path, expected) in [
        ("../x.md", DocError::InvalidPath),
        ("docs/../README.md", DocError::InvalidPath),
        ("/etc/x.md", DocError::InvalidPath),
        ("C:/x.md", DocError::InvalidPath),
        ("docs/a/x.txt", DocError::InvalidPath),
        ("", DocError::InvalidPath),
        ("README.md", DocError::OutsideRoot),
        ("docs/dir.md", DocError::InvalidPath),
        ("docs/none/x.md", DocError::ParentMissing),
    ] {
        assert_eq!(resolve(dir.path(), "docs", path), Err(expected), "{path}");
    }
    assert_eq!(
        resolve(dir.path(), "../docs", "docs/a/x.md"),
        Err(DocError::OutsideRoot)
    );
}

#[cfg(windows)]
fn junction(link: &Path, destination: &Path) {
    let result = std::process::Command::new("cmd.exe")
        .args(["/c", "mklink", "/J"])
        .arg(link)
        .arg(destination)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[cfg(windows)]
#[test]
fn resolve_rejects_junction_leaving_docs_root() {
    let dir = project();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("y.md"), "secret").unwrap();
    junction(&dir.path().join("docs").join("link"), outside.path());
    assert_eq!(
        resolve(dir.path(), "docs", "docs/link/y.md"),
        Err(DocError::OutsideRoot)
    );
    assert_eq!(
        resolve(dir.path(), "docs", "docs/link/new.md"),
        Err(DocError::OutsideRoot)
    );
}

#[test]
fn read_normalizes_line_breaks_and_remembers_format() {
    let dir = project();
    let file = dir.path().join("docs/crlf.md");
    fs::write(&file, b"\xEF\xBB\xBF# \xD0\xBF\r\nline\r\n").unwrap();
    let out = read(&file).unwrap();
    assert_eq!(out.text, "# п\nline\n");
    assert_eq!((out.eol, out.bom, out.read_only), (Eol::Crlf, true, None));
    let lf = read(&dir.path().join("docs/index.md")).unwrap();
    assert_eq!((lf.eol, lf.bom), (Eol::Lf, false));
    assert_ne!(out.revision, lf.revision);
}

#[test]
fn read_marks_foreign_encoding_oversize_and_readonly_files_read_only() {
    let dir = project();
    let latin = dir.path().join("docs/latin.md");
    fs::write(&latin, b"caf\xE9").unwrap();
    assert_eq!(
        read(&latin).unwrap().read_only,
        Some(ReadOnlyReason::NotUtf8)
    );

    let big = dir.path().join("docs/big.md");
    fs::write(&big, vec![b'a'; MAX_EDITABLE as usize + 1]).unwrap();
    assert_eq!(
        read(&big).unwrap().read_only,
        Some(ReadOnlyReason::TooLarge)
    );

    let locked = dir.path().join("docs/locked.md");
    fs::write(&locked, "x").unwrap();
    set_readonly(&locked, true);
    assert_eq!(read(&locked).unwrap().read_only, Some(ReadOnlyReason::File));
    set_readonly(&locked, false);

    assert_eq!(
        read(&dir.path().join("docs/none.md")),
        Err(DocError::NotFound)
    );
}

impl PartialEq for ReadOut {
    fn eq(&self, other: &Self) -> bool {
        self.revision == other.revision
    }
}

fn set_readonly(file: &Path, value: bool) {
    let mut permissions = fs::metadata(file).unwrap().permissions();
    permissions.set_readonly(value);
    fs::set_permissions(file, permissions).unwrap();
}

#[test]
fn write_replaces_file_keeping_eol_bom_and_leaves_no_temp_file() {
    let dir = project();
    let file = dir.path().join("docs/crlf.md");
    fs::write(&file, b"\xEF\xBB\xBFold\r\n").unwrap();
    let before = read(&file).unwrap();
    let outcome = write(
        &file,
        "new\ntext\n",
        Some(&before.revision),
        before.eol,
        before.bom,
    )
    .unwrap();
    let bytes = fs::read(&file).unwrap();
    assert_eq!(bytes, b"\xEF\xBB\xBFnew\r\ntext\r\n");
    assert_eq!(
        outcome,
        WriteOutcome::Saved {
            revision: revision_of(&bytes)
        }
    );
    assert_eq!(revision(&file).unwrap(), Some(revision_of(&bytes)));
    assert!(temp_files(&dir.path().join("docs")).is_empty());
}

#[test]
fn write_refuses_when_disk_changed_and_keeps_both_versions() {
    let dir = project();
    let file = dir.path().join("docs/index.md");
    let base = read(&file).unwrap().revision;
    fs::write(&file, "# external\n").unwrap();
    let outcome = write(&file, "# mine\n", Some(&base), Eol::Lf, false).unwrap();
    assert_eq!(
        outcome,
        WriteOutcome::Conflict {
            current: Some(revision_of(b"# external\n"))
        }
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "# external\n");
    assert!(temp_files(&dir.path().join("docs")).is_empty());
}

#[test]
fn write_reports_deleted_file_as_conflict_without_recreating_it() {
    let dir = project();
    let file = dir.path().join("docs/index.md");
    let base = read(&file).unwrap().revision;
    fs::remove_file(&file).unwrap();
    assert_eq!(
        write(&file, "# mine\n", Some(&base), Eol::Lf, false).unwrap(),
        WriteOutcome::Conflict { current: None }
    );
    assert!(!file.exists());
}

#[test]
fn write_creates_new_document_on_first_edit_and_never_overwrites_a_late_one() {
    let dir = project();
    let file = resolve(dir.path(), "docs", "docs/a/index.md").unwrap();
    let outcome = write(&file, "# new\n", None, Eol::Lf, false).unwrap();
    assert!(matches!(outcome, WriteOutcome::Saved { .. }));
    assert_eq!(fs::read_to_string(&file).unwrap(), "# new\n");

    let again = write(&file, "# other\n", None, Eol::Lf, false).unwrap();
    assert_eq!(
        again,
        WriteOutcome::Conflict {
            current: Some(revision_of(b"# new\n"))
        }
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "# new\n");
    assert!(temp_files(&dir.path().join("docs/a")).is_empty());
}

#[test]
fn write_to_readonly_file_fails_clearly_and_succeeds_after_it_is_released() {
    let dir = project();
    let file = dir.path().join("docs/index.md");
    let base = read(&file).unwrap().revision;
    set_readonly(&file, true);
    assert_eq!(
        write(&file, "# mine\n", Some(&base), Eol::Lf, false),
        Err(DocError::ReadOnly)
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "# i\n");
    set_readonly(&file, false);
    assert!(matches!(
        write(&file, "# mine\n", Some(&base), Eol::Lf, false).unwrap(),
        WriteOutcome::Saved { .. }
    ));
}

#[test]
fn write_without_parent_folder_fails_and_creates_nothing() {
    let dir = project();
    let file = dir.path().join("docs/missing/x.md");
    assert_eq!(
        write(&file, "x", None, Eol::Lf, false),
        Err(DocError::ParentMissing)
    );
    assert!(!dir.path().join("docs/missing").exists());
}
