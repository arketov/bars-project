//! `BARS_INSTRUCTION.md` в корне документации проекта (Т-8, `docs/features/docs-convention/bars-instruction.md`).
//!
//! Текст зашит в bars. Файл сверяется по хешу с текстом этой версии и перезаписывается, если
//! отличается или отсутствует; совпадающий не трогается.

use std::fs;
use std::io;
use std::path::Path;

/// Имя файла инструкции.
pub const INSTRUCTION_FILE: &str = "BARS_INSTRUCTION.md";

/// Текст инструкции этой версии bars.
pub const INSTRUCTION_TEXT: &str = include_str!("../assets/BARS_INSTRUCTION.md");

/// Приводит файл инструкции проекта к тексту этой версии. Корень документации, которого нет,
/// не создаётся: проект без документации bars ничем не засоряет. `true` — файл записан.
pub fn sync(project: &Path, docs_root: &str) -> io::Result<bool> {
    let dir = project.join(docs_root);
    if !dir.is_dir() {
        return Ok(false);
    }
    let path = dir.join(INSTRUCTION_FILE);
    let current = match fs::read(&path) {
        Ok(bytes) => Some(blake3::hash(&bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    if current == Some(blake3::hash(INSTRUCTION_TEXT.as_bytes())) {
        return Ok(false);
    }
    fs::write(&path, INSTRUCTION_TEXT)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_missing_file_and_fixes_edited_one() {
        let project = tempfile::tempdir().unwrap();
        fs::create_dir(project.path().join("docs")).unwrap();
        let file = project.path().join("docs").join(INSTRUCTION_FILE);

        assert!(sync(project.path(), "docs").unwrap());
        assert_eq!(fs::read_to_string(&file).unwrap(), INSTRUCTION_TEXT);
        assert!(!sync(project.path(), "docs").unwrap(), "совпадает по хешу");

        fs::write(&file, "правка пользователя").unwrap();
        assert!(sync(project.path(), "docs").unwrap());
        assert_eq!(fs::read_to_string(&file).unwrap(), INSTRUCTION_TEXT);
    }

    #[test]
    fn missing_docs_root_is_not_created() {
        let project = tempfile::tempdir().unwrap();
        assert!(!sync(project.path(), "docs").unwrap());
        assert!(!project.path().join("docs").exists());
    }
}
