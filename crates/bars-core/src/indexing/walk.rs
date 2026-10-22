//! Обход файлов проекта: какие `.md` индексируются и к какому модулю они относятся
//! (`docs/features/indexing/updates.md`, Т-7).
//!
//! Только диск: обход перечисляет файлы и не читает их содержимое. Сравнение с индексом и хеш
//! blake3 — дело индексации, которой нужен индекс.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

/// Имя модуля для файлов прямо в корне документации.
pub const ROOT_MODULE: &str = ".";

/// Файл, который bars кладёт в корень документации сам (Т-8); в индекс он не попадает.
const INSTRUCTION_FILE: &str = "BARS_INSTRUCTION.md";

/// Найденный документ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocFile {
    /// Путь от корня проекта через `/`: он же ключ документа в индексе и аргумент нарезки.
    pub path: String,
    /// Подпапка первого уровня под корнем документации или [`ROOT_MODULE`].
    pub module: String,
    pub size: u64,
    /// `None`, если файловая система не отдаёт время изменения.
    pub modified: Option<SystemTime>,
    /// Путь на диске, по которому файл читается.
    pub fs_path: PathBuf,
}

/// То, что не удалось обойти. Остальные документы от этого не страдают.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkProblem {
    /// Путь от корня проекта; если имя не Unicode — с заменой нечитаемых символов.
    pub path: String,
    pub reason: String,
}

/// Результат обхода: документы по возрастанию пути и проблемы по возрастанию пути.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Walk {
    pub files: Vec<DocFile>,
    /// Папки корня документации без точки в имени, включая пустые: путь от корня проекта через `/`,
    /// по возрастанию. Для дерева окна (Т-33); на индексацию не влияют.
    pub dirs: Vec<String>,
    pub problems: Vec<WalkProblem>,
}

/// Корень документации непригоден; обход не начат.
#[derive(Debug, thiserror::Error)]
pub enum WalkError {
    #[error("корень документации `{0}` должен быть относительным путём без `..`")]
    InvalidDocsRoot(String),
    #[error("корень документации `{0}` не найден или не папка: {1}")]
    DocsRootUnavailable(String, io::Error),
}

/// Файл не прочитался как UTF-8-текст.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("файл не читается: {0}")]
    Io(#[from] io::Error),
    #[error("файл не в кодировке UTF-8")]
    NotUtf8,
}

/// Обходит `docs_root` (относительно `project_root`) и собирает индексируемые `.md`.
pub fn walk_docs(project_root: &Path, docs_root: &Path) -> Result<Walk, WalkError> {
    let docs_prefix = docs_prefix(docs_root)?;
    let start = project_root.join(docs_root);
    let unavailable =
        |error| WalkError::DocsRootUnavailable(docs_root.display().to_string(), error);
    if !fs::metadata(&start).map_err(unavailable)?.is_dir() {
        return Err(unavailable(io::Error::other("это не папка")));
    }

    let mut walk = Walk::default();
    // Папка на диске и её путь внутри корня документации (пустой для самого корня).
    let mut pending = vec![(start, String::new())];
    while let Some((dir, inner)) = pending.pop() {
        let project_path = |name: &str| join(&join(&docs_prefix, &inner), name);
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) => {
                walk.problems.push(WalkProblem {
                    path: join(&docs_prefix, &inner),
                    reason: format!("папка не читается: {error}"),
                });
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    walk.problems.push(WalkProblem {
                        path: join(&docs_prefix, &inner),
                        reason: format!("запись папки не читается: {error}"),
                    });
                    continue;
                }
            };
            let Ok(name) = entry.file_name().into_string() else {
                walk.problems.push(WalkProblem {
                    path: project_path(&entry.file_name().to_string_lossy()),
                    reason: "имя не является корректным Unicode".to_string(),
                });
                continue;
            };
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(error) => {
                    walk.problems.push(WalkProblem {
                        path: project_path(&name),
                        reason: format!("не читаются свойства: {error}"),
                    });
                    continue;
                }
            };
            let kind = metadata.file_type();
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                if !name.starts_with('.') {
                    walk.dirs.push(project_path(&name));
                    pending.push((entry.path(), join(&inner, &name)));
                }
            } else if kind.is_file() && is_indexed_name(&name, inner.is_empty()) {
                walk.files.push(DocFile {
                    path: project_path(&name),
                    module: inner
                        .split('/')
                        .next()
                        .filter(|m| !m.is_empty())
                        .unwrap_or(ROOT_MODULE)
                        .to_string(),
                    size: metadata.len(),
                    modified: metadata.modified().ok(),
                    fs_path: entry.path(),
                });
            }
        }
    }
    walk.files.sort_by(|a, b| a.path.cmp(&b.path));
    walk.dirs.sort();
    walk.problems.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(walk)
}

/// Читает документ строго как UTF-8. BOM остаётся в тексте: его снимает нарезка.
pub fn read_utf8(file: &DocFile) -> Result<String, ReadError> {
    read_utf8_path(&file.fs_path)
}

/// То же по пути на диске: для документа, который не обязан лежать в корне документации.
pub fn read_utf8_path(path: &Path) -> Result<String, ReadError> {
    String::from_utf8(fs::read(path)?).map_err(|_| ReadError::NotUtf8)
}

/// Корень документации как префикс путей проекта: `docs`, `a/b` или пусто для `.`.
fn docs_prefix(docs_root: &Path) -> Result<String, WalkError> {
    let invalid = || WalkError::InvalidDocsRoot(docs_root.display().to_string());
    let mut parts = Vec::new();
    for component in docs_root.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => parts.push(part.to_str().ok_or_else(invalid)?),
            _ => return Err(invalid()),
        }
    }
    Ok(parts.join("/"))
}

fn join(base: &str, name: &str) -> String {
    match (base.is_empty(), name.is_empty()) {
        (true, _) => name.to_string(),
        (_, true) => base.to_string(),
        _ => format!("{base}/{name}"),
    }
}

/// `.md` без учёта регистра, не считая служебного файла bars в корне документации.
fn is_indexed_name(name: &str, in_docs_root: bool) -> bool {
    let is_markdown = name.len() > 3
        && name.is_char_boundary(name.len() - 3)
        && name[name.len() - 3..].eq_ignore_ascii_case(".md");
    is_markdown && !(in_docs_root && name.eq_ignore_ascii_case(INSTRUCTION_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(root: &Path, path: &str, content: &[u8]) {
        let full = root.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, content).unwrap();
    }

    fn paths(walk: &Walk) -> Vec<&str> {
        walk.files.iter().map(|f| f.path.as_str()).collect()
    }

    fn module_of<'a>(walk: &'a Walk, path: &str) -> &'a str {
        &walk.files.iter().find(|f| f.path == path).unwrap().module
    }

    /// Папка-ссылка: символическая на Unix, junction на Windows. `false`, если создать не вышло.
    fn link_dir(target: &Path, link: &Path) -> bool {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
        #[cfg(windows)]
        {
            // `mklink` не принимает прямые слэши в путях.
            let backslashed = |path: &Path| path.to_string_lossy().replace('/', "\\");
            std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(backslashed(link))
                .arg(backslashed(target))
                .output()
                .is_ok_and(|out| out.status.success())
        }
    }

    #[test]
    fn modules_follow_first_level_folders() {
        let dir = tempfile::tempdir().unwrap();
        for path in [
            "docs/Index.md",
            "docs/a/x.md",
            "docs/a/b/y.md",
            "docs/c/z.md",
        ] {
            touch(dir.path(), path, b"# t");
        }
        let walk = walk_docs(dir.path(), Path::new("docs")).unwrap();

        assert_eq!(
            paths(&walk),
            [
                "docs/Index.md",
                "docs/a/b/y.md",
                "docs/a/x.md",
                "docs/c/z.md"
            ]
        );
        assert_eq!(module_of(&walk, "docs/Index.md"), ".");
        assert_eq!(module_of(&walk, "docs/a/x.md"), "a");
        assert_eq!(module_of(&walk, "docs/a/b/y.md"), "a");
        assert_eq!(module_of(&walk, "docs/c/z.md"), "c");
        assert!(walk.problems.is_empty());
    }

    #[test]
    fn empty_folders_are_listed_but_hidden_and_linked_ones_are_not() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "docs/a/x.md", b"# t");
        touch(dir.path(), "docs/.hidden/y.md", b"# t");
        fs::create_dir_all(dir.path().join("docs/empty/inner")).unwrap();
        touch(dir.path(), "outside/secret.md", b"# t");
        let linked = link_dir(&dir.path().join("outside"), &dir.path().join("docs/link"));
        let walk = walk_docs(dir.path(), Path::new("docs")).unwrap();

        assert_eq!(walk.dirs, ["docs/a", "docs/empty", "docs/empty/inner"]);
        assert!(linked || !cfg!(windows));
    }

    #[test]
    fn nested_docs_root_gives_paths_from_project_root() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "documentation/guide/x.md", b"# t");
        touch(dir.path(), "docs/other.md", b"# t");
        let walk = walk_docs(dir.path(), Path::new("./documentation")).unwrap();

        assert_eq!(paths(&walk), ["documentation/guide/x.md"]);
        assert_eq!(module_of(&walk, "documentation/guide/x.md"), "guide");
    }

    #[test]
    fn project_root_as_docs_root_skips_dot_folders() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "README.md", b"# t");
        touch(dir.path(), "a/x.md", b"# t");
        touch(dir.path(), ".bars/notes.md", b"# t");
        let walk = walk_docs(dir.path(), Path::new(".")).unwrap();

        assert_eq!(paths(&walk), ["README.md", "a/x.md"]);
        assert_eq!(module_of(&walk, "README.md"), ".");
    }

    #[test]
    fn exclusions() {
        let dir = tempfile::tempdir().unwrap();
        for path in [
            "docs/BARS_INSTRUCTION.md",
            "docs/a/BARS_INSTRUCTION.md",
            "docs/.hidden/x.md",
            "docs/a/.git/y.md",
            "docs/.notes.md",
            "docs/UPPER.MD",
            "docs/readme.txt",
            "docs/md",
            "docs/.md",
            "docs/dir.md/inner.md",
        ] {
            touch(dir.path(), path, b"# t");
        }
        let walk = walk_docs(dir.path(), Path::new("docs")).unwrap();

        assert_eq!(
            paths(&walk),
            [
                "docs/.notes.md",
                "docs/UPPER.MD",
                "docs/a/BARS_INSTRUCTION.md",
                "docs/dir.md/inner.md"
            ]
        );
    }

    #[test]
    fn instruction_file_is_skipped_in_any_case() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "docs/bars_instruction.md", b"# t");
        let walk = walk_docs(dir.path(), Path::new("docs")).unwrap();
        assert!(walk.files.is_empty());
    }

    #[test]
    fn links_are_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "outside/secret.md", b"# t");
        touch(dir.path(), "docs/a/x.md", b"# t");
        if !link_dir(&dir.path().join("outside"), &dir.path().join("docs/link")) {
            // Без прав на ссылки проверить нечего; на Windows junction права не требует.
            if cfg!(windows) {
                panic!("junction не создался");
            }
            return;
        }
        let walk = walk_docs(dir.path(), Path::new("docs")).unwrap();

        assert_eq!(paths(&walk), ["docs/a/x.md"]);
        assert!(walk.problems.is_empty());
    }

    #[test]
    fn docs_root_must_be_relative_and_inside_project() {
        let dir = tempfile::tempdir().unwrap();
        for root in ["../docs", "docs/../../x", dir.path().to_str().unwrap()] {
            let error = walk_docs(dir.path(), Path::new(root)).unwrap_err();
            assert!(matches!(error, WalkError::InvalidDocsRoot(_)), "{root}");
        }
    }

    #[test]
    fn missing_or_file_docs_root_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "plain.md", b"# t");
        for root in ["nope", "plain.md"] {
            let error = walk_docs(dir.path(), Path::new(root)).unwrap_err();
            assert!(
                matches!(error, WalkError::DocsRootUnavailable(..)),
                "{root}"
            );
        }
    }

    #[test]
    fn size_and_modified_time_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "docs/x.md", b"12345");
        let walk = walk_docs(dir.path(), Path::new("docs")).unwrap();

        assert_eq!(walk.files[0].size, 5);
        assert!(walk.files[0].modified.is_some());
    }

    #[test]
    fn read_utf8_keeps_bom_and_rejects_other_encodings() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "docs/bom.md", "\u{feff}# Заголовок".as_bytes());
        touch(dir.path(), "docs/bad.md", &[0x23, 0x20, 0xff, 0xfe]);
        let walk = walk_docs(dir.path(), Path::new("docs")).unwrap();

        // Обход не читает содержимое: нечитаемый файл он перечисляет как обычный.
        assert_eq!(paths(&walk), ["docs/bad.md", "docs/bom.md"]);
        assert!(matches!(read_utf8(&walk.files[0]), Err(ReadError::NotUtf8)));
        assert_eq!(read_utf8(&walk.files[1]).unwrap(), "\u{feff}# Заголовок");
    }

    #[test]
    fn read_utf8_reports_io_error_for_vanished_file() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "docs/x.md", b"# t");
        let walk = walk_docs(dir.path(), Path::new("docs")).unwrap();
        fs::remove_file(&walk.files[0].fs_path).unwrap();

        assert!(matches!(read_utf8(&walk.files[0]), Err(ReadError::Io(_))));
    }

    #[cfg(unix)]
    #[test]
    fn non_unicode_name_is_a_problem_not_a_failure() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "docs/ok.md", b"# t");
        let bad = dir
            .path()
            .join("docs")
            .join(OsStr::from_bytes(b"bad\xff.md"));
        fs::write(bad, b"# t").unwrap();
        let walk = walk_docs(dir.path(), Path::new("docs")).unwrap();

        assert_eq!(paths(&walk), ["docs/ok.md"]);
        assert_eq!(walk.problems.len(), 1);
        assert!(walk.problems[0].path.starts_with("docs/bad"));
    }
}
