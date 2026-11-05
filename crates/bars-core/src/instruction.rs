//! `BARS_INSTRUCTION.md` в корне документации проекта (Т-8, `docs/features/docs-convention/bars-instruction.md`).
//!
//! Текст зашит в bars. Файл кладётся, когда с проектом работает агент ([`place`]); существующий
//! сверяется по хешу и перезаписывается, если отличается ([`sync`]); совпадающий не трогается.
//! Добавление папки в окне и создание проекта файл не создают.

use std::fs;
use std::io;
use std::path::Path;

/// Имя файла инструкции.
pub const INSTRUCTION_FILE: &str = "BARS_INSTRUCTION.md";

/// Текст инструкции этой версии bars.
pub const INSTRUCTION_TEXT: &str = include_str!("../assets/BARS_INSTRUCTION.md");

/// Сверяет уже существующий файл инструкции с текстом этой версии и перезаписывает при
/// расхождении. Отсутствующий файл не создаётся: он появляется только когда с проектом работает
/// агент ([`place`], Т-8). Корень документации, которого нет, тоже не создаётся.
/// `true` — файл записан.
pub fn sync(project: &Path, docs_root: &str) -> io::Result<bool> {
    write_if_needed(project, docs_root, false)
}

/// Кладёт инструкцию, когда с проектом начал работать агент (хук сессии, первый вызов MCP):
/// отсутствующий файл записывается, существующий сверяется по хешу, как в [`sync`].
pub fn place(project: &Path, docs_root: &str) -> io::Result<bool> {
    write_if_needed(project, docs_root, true)
}

fn write_if_needed(project: &Path, docs_root: &str, create: bool) -> io::Result<bool> {
    let dir = project.join(docs_root);
    if !dir.is_dir() {
        return Ok(false);
    }
    let path = dir.join(INSTRUCTION_FILE);
    let current = match fs::read(&path) {
        Ok(bytes) => Some(blake3::hash(&bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if !create {
                return Ok(false);
            }
            None
        }
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
    fn sync_does_not_create_a_missing_file_but_place_does() {
        let project = tempfile::tempdir().unwrap();
        fs::create_dir(project.path().join("docs")).unwrap();
        let file = project.path().join("docs").join(INSTRUCTION_FILE);

        assert!(!sync(project.path(), "docs").unwrap());
        assert!(!file.exists(), "окно и проверка без агента файл не создают");
        assert!(place(project.path(), "docs").unwrap());
        assert_eq!(fs::read_to_string(&file).unwrap(), INSTRUCTION_TEXT);
        assert!(!place(project.path(), "docs").unwrap());
    }

    #[test]
    fn fixes_an_existing_edited_file_by_hash() {
        let project = tempfile::tempdir().unwrap();
        fs::create_dir(project.path().join("docs")).unwrap();
        let file = project.path().join("docs").join(INSTRUCTION_FILE);

        assert!(place(project.path(), "docs").unwrap());
        assert_eq!(fs::read_to_string(&file).unwrap(), INSTRUCTION_TEXT);
        assert!(!sync(project.path(), "docs").unwrap(), "совпадает по хешу");

        fs::write(&file, "правка пользователя").unwrap();
        assert!(sync(project.path(), "docs").unwrap());
        assert_eq!(fs::read_to_string(&file).unwrap(), INSTRUCTION_TEXT);
    }

    #[test]
    fn custom_root_sync_preserves_the_projects_layout() {
        let project = tempfile::tempdir().unwrap();
        let root = project.path().join("manual");
        fs::create_dir_all(root.join("existing-notes")).unwrap();
        let source = root.join("existing-notes/bugs.md");
        fs::write(&source, "# Project bugs\n\nKeep this location.\n").unwrap();
        let original = fs::read(&source).unwrap();

        assert!(place(project.path(), "manual").unwrap());
        assert_eq!(fs::read(&source).unwrap(), original);
        assert!(!project.path().join("docs").exists());
        for suggested in ["backlog", "features", "adr", "chats"] {
            assert!(!root.join(suggested).exists());
        }
        assert_eq!(
            fs::read_to_string(root.join(INSTRUCTION_FILE)).unwrap(),
            INSTRUCTION_TEXT
        );
        assert!(!sync(project.path(), "manual").unwrap());
    }

    #[test]
    fn missing_docs_root_is_not_created() {
        let project = tempfile::tempdir().unwrap();
        assert!(!sync(project.path(), "docs").unwrap());
        assert!(!place(project.path(), "docs").unwrap());
        assert!(!project.path().join("docs").exists());
    }
}
