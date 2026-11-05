//! Выбор корня документации при добавлении и создании проекта
//! (`docs/features/projects/projects.md`, Т-7, Т-35).
//!
//! Живёт в протоколе, потому что нужен и `bars-cli` (ядра `bars-core` у него нет), и окну. Выбор
//! записывается в `.bars/config.toml` до регистрации: воркер существующий файл не трогает.
//! Проект, у которого `config.toml` уже есть, не меняется.

use std::fs;
use std::io;
use std::path::Path;

/// Корень «папка `docs/`»: то же значение, что `DEFAULT_DOCS_ROOT` в `bars-core`.
const DOCS_FOLDER: &str = "docs";
/// Корень «вся папка».
const WHOLE_FOLDER: &str = ".";
/// Имя корневого документа, который создаёт создание проекта.
const INDEX_FILE: &str = "index.md";

/// Что считать документацией папки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootChoice {
    /// `docs_root = "."`: заметки, инструкции, руководства.
    WholeFolder,
    /// `docs_root = "docs"`: репозиторий кода.
    DocsFolder,
}

impl RootChoice {
    /// Значение ключа `docs_root`.
    pub fn docs_root(self) -> &'static str {
        match self {
            Self::WholeFolder => WHOLE_FOLDER,
            Self::DocsFolder => DOCS_FOLDER,
        }
    }

    /// Умолчание без выбора человека: `docs/`, если она есть, иначе вся папка.
    pub fn suggested(folder: &Path) -> Self {
        if folder.join(DOCS_FOLDER).is_dir() {
            Self::DocsFolder
        } else {
            Self::WholeFolder
        }
    }

    /// Разбор значения `--root`: `docs` или `all` (и `.` как запись корня).
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_lowercase().as_str() {
            "docs" => Some(Self::DocsFolder),
            "all" | "." => Some(Self::WholeFolder),
            _ => None,
        }
    }
}

/// Путь к `config.toml` проекта.
fn config_path(folder: &Path) -> std::path::PathBuf {
    folder.join(".bars").join("config.toml")
}

/// Текст `config.toml` нового проекта с выбранным корнем.
pub fn initial_config_text(choice: RootChoice) -> String {
    format!(
        "# Настройки проекта bars. Что можно задать — docs/features/settings/settings.md.\n\
         docs_root = \"{}\"\n",
        choice.docs_root()
    )
}

/// Записывает выбор корня в `.bars/config.toml`, если файла ещё нет. Без выбора действует
/// [`RootChoice::suggested`]. Возвращает записанный выбор или `None`, если проект уже настроен
/// и остался как был.
pub fn prepare_project(
    folder: &Path,
    choice: Option<RootChoice>,
) -> io::Result<Option<RootChoice>> {
    let file = config_path(folder);
    if file.exists() {
        return Ok(None);
    }
    let choice = choice.unwrap_or_else(|| RootChoice::suggested(folder));
    write_config(&file, choice)?;
    Ok(Some(choice))
}

/// Создание проекта (Т-35): корень документации (папка `docs/` только при выборе `docs/`),
/// `index.md` в нём и `.bars/config.toml`. Регистрацию выполняет вызывающий. Папка, у которой
/// уже есть `config.toml`, — уже проект: отказ.
pub fn create_project(folder: &Path, choice: Option<RootChoice>) -> io::Result<RootChoice> {
    let file = config_path(folder);
    if file.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "папка уже является проектом bars",
        ));
    }
    let choice = choice.unwrap_or_else(|| RootChoice::suggested(folder));
    let root = folder.join(choice.docs_root());
    fs::create_dir_all(&root)?;
    if !has_index(&root)? {
        fs::write(root.join(INDEX_FILE), index_text(folder))?;
    }
    write_config(&file, choice)?;
    Ok(choice)
}

fn write_config(file: &Path, choice: RootChoice) -> io::Result<()> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(file, initial_config_text(choice))
}

/// Есть ли в корне документации файл `index.md` в любом регистре.
fn has_index(root: &Path) -> io::Result<bool> {
    for entry in fs::read_dir(root)? {
        if entry?
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(INDEX_FILE)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn index_text(folder: &Path) -> String {
    let name = folder.file_name().map_or_else(
        || "Проект".to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    format!("---\ntitle: {name}\n---\n\n# {name}\n")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// Временная папка, удаляемая при выходе из области видимости.
    struct Temp(PathBuf);

    impl Temp {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "bars-root-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn config(folder: &Path) -> String {
        fs::read_to_string(config_path(folder)).unwrap()
    }

    #[test]
    fn docs_folder_is_suggested_when_it_exists() {
        let dir = Temp::new();
        assert_eq!(RootChoice::suggested(dir.path()), RootChoice::WholeFolder);
        fs::create_dir(dir.path().join("docs")).unwrap();
        assert_eq!(RootChoice::suggested(dir.path()), RootChoice::DocsFolder);
    }

    #[test]
    fn root_option_parses_docs_and_all() {
        assert_eq!(RootChoice::parse("docs"), Some(RootChoice::DocsFolder));
        assert_eq!(RootChoice::parse(" ALL "), Some(RootChoice::WholeFolder));
        assert_eq!(RootChoice::parse("."), Some(RootChoice::WholeFolder));
        assert_eq!(RootChoice::parse("x"), None);
    }

    #[test]
    fn prepare_writes_the_default_without_a_choice() {
        let notes = Temp::new();
        assert_eq!(
            prepare_project(notes.path(), None).unwrap(),
            Some(RootChoice::WholeFolder)
        );
        assert!(config(notes.path()).contains("docs_root = \".\""));

        let repo = Temp::new();
        fs::create_dir(repo.path().join("docs")).unwrap();
        prepare_project(repo.path(), None).unwrap();
        assert!(config(repo.path()).contains("docs_root = \"docs\""));
    }

    #[test]
    fn explicit_choice_beats_the_suggestion() {
        let dir = Temp::new();
        fs::create_dir(dir.path().join("docs")).unwrap();
        prepare_project(dir.path(), Some(RootChoice::WholeFolder)).unwrap();
        assert!(config(dir.path()).contains("docs_root = \".\""));
    }

    #[test]
    fn configured_project_is_left_unchanged() {
        let dir = Temp::new();
        fs::create_dir(dir.path().join(".bars")).unwrap();
        fs::write(config_path(dir.path()), "docs_root = \"manual\"\n").unwrap();
        assert_eq!(
            prepare_project(dir.path(), Some(RootChoice::WholeFolder)).unwrap(),
            None
        );
        assert_eq!(config(dir.path()), "docs_root = \"manual\"\n");
    }

    #[test]
    fn create_makes_docs_folder_only_for_the_docs_choice() {
        let repo = Temp::new();
        create_project(repo.path(), Some(RootChoice::DocsFolder)).unwrap();
        assert!(repo.path().join("docs/index.md").is_file());
        assert!(config(repo.path()).contains("docs_root = \"docs\""));

        let notes = Temp::new();
        create_project(notes.path(), Some(RootChoice::WholeFolder)).unwrap();
        assert!(notes.path().join("index.md").is_file());
        assert!(!notes.path().join("docs").exists());
        assert!(config(notes.path()).contains("docs_root = \".\""));
    }

    #[test]
    fn create_keeps_an_existing_index_in_any_case() {
        let dir = Temp::new();
        fs::write(dir.path().join("Index.md"), "# Мой\n").unwrap();
        create_project(dir.path(), Some(RootChoice::WholeFolder)).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("Index.md")).unwrap(),
            "# Мой\n"
        );
    }

    #[test]
    fn create_refuses_an_existing_project() {
        let dir = Temp::new();
        prepare_project(dir.path(), None).unwrap();
        let error = create_project(dir.path(), None).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    }
}
