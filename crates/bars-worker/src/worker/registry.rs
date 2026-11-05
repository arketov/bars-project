//! Реестр проектов: `~/.bars/projects.toml` и поиск корня проекта (`docs/architecture/projects.md`,
//! Т-5, Т-15).
//!
//! Реестр хранит пути, отображаемые имена и время регистрации. Что с проектом происходит дальше — пропал он
//! или вернулся, — решает воркер.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use super::timefmt::rfc3339;

/// Имя файла реестра в `~/.bars`.
pub const REGISTRY_FILE: &str = "projects.toml";

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("путь проекта должен быть абсолютным: {0}")]
    NotAbsolute(PathBuf),
    #[error("не удалось прочитать {file}: {source}")]
    Read { file: PathBuf, source: io::Error },
    #[error("ошибка в {file}: {message}")]
    Parse { file: PathBuf, message: String },
    #[error("не удалось записать {file}: {source}")]
    Write { file: PathBuf, source: io::Error },
}

/// Запись реестра.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectEntry {
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// RFC 3339, UTC.
    pub registered_at: String,
}

impl ProjectEntry {
    pub fn display_name(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| project_name(&self.path))
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RegistryFile {
    #[serde(default, rename = "project")]
    projects: Vec<ProjectEntry>,
}

/// Почему строка не нашла проект.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResolveError {
    #[error("проект `{0}` не зарегистрирован")]
    NotFound(String),
    /// Имя совпало у нескольких проектов: нужен путь (`docs/architecture/projects.md`).
    #[error("имя `{name}` носят несколько проектов: укажите путь")]
    Ambiguous { name: String, paths: Vec<PathBuf> },
}

#[derive(Debug)]
pub struct Registry {
    file: PathBuf,
    projects: Vec<ProjectEntry>,
}

impl Registry {
    /// Читает `projects.toml`; нет файла — реестр пуст. Битый файл — ошибка, а не пустой реестр:
    /// следующая запись иначе стёрла бы список проектов.
    pub fn load(bars_home: &Path) -> Result<Self, RegistryError> {
        let file = bars_home.join(REGISTRY_FILE);
        let projects = match fs::read_to_string(&file) {
            Ok(text) => {
                toml::from_str::<RegistryFile>(&text)
                    .map_err(|error| RegistryError::Parse {
                        file: file.clone(),
                        message: error.to_string(),
                    })?
                    .projects
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(source) => return Err(RegistryError::Read { file, source }),
        };
        Ok(Self { file, projects })
    }

    pub fn entries(&self) -> &[ProjectEntry] {
        &self.projects
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        self.projects
            .iter()
            .map(|entry| entry.path.clone())
            .collect()
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.find(path).is_some()
    }

    pub fn find(&self, path: &Path) -> Option<&ProjectEntry> {
        let key = path_key(path);
        self.projects
            .iter()
            .find(|entry| path_key(&entry.path) == key)
    }

    /// Добавляет проект и записывает файл. `Ok(false)` — проект уже был, реестр не менялся.
    pub fn add(&mut self, path: &Path) -> Result<bool, RegistryError> {
        let path = normalize(path)?;
        if self.contains(&path) {
            return Ok(false);
        }
        self.projects.push(ProjectEntry {
            path,
            name: None,
            registered_at: rfc3339(SystemTime::now()),
        });
        if let Err(error) = self.save() {
            self.projects.pop();
            return Err(error);
        }
        Ok(true)
    }

    /// Убирает проект и записывает файл. `Ok(false)` — такого проекта не было.
    pub fn remove(&mut self, path: &Path) -> Result<bool, RegistryError> {
        let key = path_key(path);
        let Some(position) = self
            .projects
            .iter()
            .position(|entry| path_key(&entry.path) == key)
        else {
            return Ok(false);
        };
        let removed = self.projects.remove(position);
        if let Err(error) = self.save() {
            self.projects.insert(position, removed);
            return Err(error);
        }
        Ok(true)
    }

    /// Меняет только имя; сбой записи откатывает изменение в памяти.
    pub fn rename(&mut self, path: &Path, name: String) -> Result<(), RegistryError> {
        let key = path_key(path);
        let Some(position) = self
            .projects
            .iter()
            .position(|entry| path_key(&entry.path) == key)
        else {
            return Err(RegistryError::Parse {
                file: self.file.clone(),
                message: "проект не зарегистрирован".into(),
            });
        };
        let previous = self.projects[position].name.replace(name);
        if let Err(error) = self.save() {
            self.projects[position].name = previous;
            return Err(error);
        }
        Ok(())
    }

    /// Проект по пути или отображаемому имени. Путь точнее имени и выигрывает.
    pub fn resolve(&self, selector: &str) -> Result<&ProjectEntry, ResolveError> {
        let as_path = Path::new(selector);
        if as_path.is_absolute()
            && let Some(entry) = self.find(as_path)
        {
            return Ok(entry);
        }
        let named: Vec<&ProjectEntry> = self
            .projects
            .iter()
            .filter(|entry| entry.display_name() == selector)
            .collect();
        match named.as_slice() {
            [] => Err(ResolveError::NotFound(selector.to_string())),
            [entry] => Ok(entry),
            _ => Err(ResolveError::Ambiguous {
                name: selector.to_string(),
                paths: named.iter().map(|entry| entry.path.clone()).collect(),
            }),
        }
    }

    /// Имя носят несколько проектов: окно и `projects` показывают такие с путём.
    pub fn has_duplicate_name(&self, path: &Path) -> bool {
        let Some(entry) = self.find(path) else {
            return false;
        };
        let name = entry.display_name();
        self.projects
            .iter()
            .filter(|entry| entry.display_name() == name)
            .count()
            > 1
    }

    pub fn project_for_cwd(&self, cwd: &Path) -> Option<&ProjectEntry> {
        cwd.ancestors()
            .find_map(|dir| self.find(dir))
            .or_else(|| self.project_of_worktree(cwd))
    }

    /// Проект основного checkout для `cwd` внутри git worktree вне проекта: корень рабочей копии
    /// — ближайшая вверх папка с файлом `.git`, основной checkout — по его `gitdir:` (OQ-289).
    fn project_of_worktree(&self, cwd: &Path) -> Option<&ProjectEntry> {
        let main = main_checkout_of_worktree(cwd)?;
        main.ancestors().find_map(|dir| self.find(dir))
    }

    pub fn discover_root(&self, cwd: &Path) -> Discovered {
        for dir in cwd.ancestors() {
            if self.contains(dir) {
                return Discovered {
                    root: dir.to_path_buf(),
                    registered: true,
                };
            }
            if has_project_bars(dir) {
                // Git worktree вне проекта не регистрируется отдельно, если основной checkout
                // уже в реестре (OQ-289, OQ-291).
                return self.project_of_worktree(cwd).map_or_else(
                    || Discovered {
                        root: dir.to_path_buf(),
                        registered: false,
                    },
                    |found| Discovered {
                        root: found.path.clone(),
                        registered: true,
                    },
                );
            }
        }
        if let Some(found) = self.project_of_worktree(cwd) {
            return Discovered {
                root: found.path.clone(),
                registered: true,
            };
        }
        let root = cwd
            .ancestors()
            .find(|dir| dir.join(".git").exists())
            .unwrap_or(cwd);
        Discovered {
            root: root.to_path_buf(),
            registered: false,
        }
    }

    fn save(&self) -> Result<(), RegistryError> {
        let write = |source| RegistryError::Write {
            file: self.file.clone(),
            source,
        };
        let text = toml::to_string_pretty(&RegistryFile {
            projects: self.projects.clone(),
        })
        .map_err(|error| write(io::Error::other(error)))?;
        if let Some(parent) = self.file.parent() {
            fs::create_dir_all(parent).map_err(write)?;
        }
        // Запись во временный файл и переименование: сбой посреди записи не портит реестр.
        let mut temp = self.file.clone().into_os_string();
        temp.push(".tmp");
        let temp = PathBuf::from(temp);
        fs::write(&temp, text).map_err(write)?;
        fs::rename(&temp, &self.file).map_err(|source| {
            let _ = fs::remove_file(&temp);
            write(source)
        })
    }
}

/// Корень, найденный по `cwd`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovered {
    pub root: PathBuf,
    /// Папка уже в реестре. Иначе хук регистрирует её, если в ней есть документация.
    pub registered: bool,
}

/// Имя проекта — имя папки корня (у корня диска — весь путь).
pub fn project_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// В папке есть `.bars/` проекта: с `config.toml` или индексом. Домашняя `.bars` пользователя
/// (`settings.toml`, `projects.toml`, модели) проектом не делает свою родительскую папку.
fn has_project_bars(dir: &Path) -> bool {
    let bars = dir.join(".bars");
    bars.join("config.toml").is_file() || bars.join("index.sqlite").is_file()
}

/// Основной checkout для `cwd` в git worktree: ближайшая вверх папка с файлом `.git` (не папкой)
/// с `gitdir: <путь>`, а путь ведёт в `<репозиторий>/.git/worktrees/<имя>`. Относительный `gitdir`
/// считается от корня рабочей копии; `commondir` уточняет общий каталог git. Подмодуль, bare-репозиторий
/// и битый файл дают `None`.
fn main_checkout_of_worktree(cwd: &Path) -> Option<PathBuf> {
    let work = cwd.ancestors().find(|dir| dir.join(".git").is_file())?;
    let text = fs::read_to_string(work.join(".git")).ok()?;
    let target = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("gitdir:"))?
        .trim();
    if target.is_empty() {
        return None;
    }
    let gitdir = normalize(&work.join(target)).ok()?;
    if gitdir.parent()?.file_name()? != "worktrees" {
        return None;
    }
    let common = match fs::read_to_string(gitdir.join("commondir")) {
        Ok(text) => normalize(&gitdir.join(text.trim())).ok()?,
        Err(_) => gitdir.parent()?.parent()?.to_path_buf(),
    };
    if common.file_name()? != ".git" {
        return None;
    }
    Some(common.parent()?.to_path_buf())
}

/// Есть ли под корнем документации хоть один `.md`: без него хук проект не регистрирует.
pub fn has_markdown(project_root: &Path, docs_root: &str) -> bool {
    fn walk(dir: &Path) -> bool {
        let Ok(entries) = fs::read_dir(dir) else {
            return false;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
            {
                return true;
            }
            let hidden = entry.file_name().to_string_lossy().starts_with('.');
            if kind.is_dir() && !hidden && walk(&path) {
                return true;
            }
        }
        false
    }
    walk(&project_root.join(docs_root))
}

/// Абсолютный путь без `.`, `..` и хвостовых разделителей.
pub fn normalize(path: &Path) -> Result<PathBuf, RegistryError> {
    if !path.is_absolute() {
        return Err(RegistryError::NotAbsolute(path.to_path_buf()));
    }
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    Ok(result)
}

/// Ключ сравнения путей: на Windows регистр букв не различается.
fn path_key(path: &Path) -> Vec<String> {
    path.components()
        .map(|component| {
            let part = component.as_os_str().to_string_lossy();
            if cfg!(windows) {
                part.to_lowercase()
            } else {
                part.into_owned()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn dir(root: &TempDir, name: &str) -> PathBuf {
        let path = root.path().join(name);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn missing_file_is_an_empty_registry() {
        let home = TempDir::new().unwrap();
        assert!(Registry::load(home.path()).unwrap().entries().is_empty());
    }

    #[test]
    fn add_persists_and_reloads() {
        let home = TempDir::new().unwrap();
        let project = dir(&home, "alpha");
        let mut registry = Registry::load(home.path()).unwrap();
        assert!(registry.add(&project).unwrap());
        assert!(!registry.add(&project).unwrap(), "повтор не дублирует");

        let reloaded = Registry::load(home.path()).unwrap();
        assert_eq!(reloaded.paths(), vec![project]);
        assert!(reloaded.entries()[0].registered_at.ends_with('Z'));
    }

    #[test]
    fn relative_path_is_refused() {
        let home = TempDir::new().unwrap();
        let mut registry = Registry::load(home.path()).unwrap();
        assert!(matches!(
            registry.add(Path::new("relative/dir")),
            Err(RegistryError::NotAbsolute(_))
        ));
    }

    #[test]
    fn broken_file_is_an_error_and_is_not_overwritten() {
        let home = TempDir::new().unwrap();
        let file = home.path().join(REGISTRY_FILE);
        fs::write(&file, "это не toml [[").unwrap();
        assert!(matches!(
            Registry::load(home.path()),
            Err(RegistryError::Parse { .. })
        ));
        assert_eq!(fs::read_to_string(&file).unwrap(), "это не toml [[");
    }

    #[test]
    fn remove_drops_the_entry_and_keeps_project_files() {
        let home = TempDir::new().unwrap();
        let project = dir(&home, "alpha");
        fs::create_dir_all(project.join(".bars")).unwrap();
        let mut registry = Registry::load(home.path()).unwrap();
        registry.add(&project).unwrap();

        assert!(registry.remove(&project).unwrap());
        assert!(!registry.remove(&project).unwrap());
        assert!(Registry::load(home.path()).unwrap().entries().is_empty());
        assert!(project.join(".bars").is_dir(), "`.bars` не удаляется (Т-5)");
    }

    #[test]
    fn dots_in_the_path_are_normalized() {
        let home = TempDir::new().unwrap();
        let project = dir(&home, "alpha");
        let mut registry = Registry::load(home.path()).unwrap();
        registry.add(&project.join("sub").join("..")).unwrap();
        assert!(registry.contains(&project));
    }

    #[test]
    fn resolve_by_path_and_by_name() {
        let home = TempDir::new().unwrap();
        let alpha = dir(&home, "alpha");
        let mut registry = Registry::load(home.path()).unwrap();
        registry.add(&alpha).unwrap();

        assert_eq!(registry.resolve("alpha").unwrap().path, alpha);
        assert_eq!(
            registry.resolve(&alpha.to_string_lossy()).unwrap().path,
            alpha
        );
        assert!(matches!(
            registry.resolve("beta"),
            Err(ResolveError::NotFound(_))
        ));
    }

    #[test]
    fn rename_persists_without_changing_identity_and_recomputes_duplicates() {
        let home = TempDir::new().unwrap();
        let first = dir(&home, "one/repo");
        let second = dir(&home, "two/repo");
        let mut registry = Registry::load(home.path()).unwrap();
        registry.add(&first).unwrap();
        registry.add(&second).unwrap();
        let registered_at = registry.find(&first).unwrap().registered_at.clone();
        assert!(registry.has_duplicate_name(&first));
        registry.rename(&first, "Мой проект".into()).unwrap();
        let mut registry = Registry::load(home.path()).unwrap();
        assert_eq!(registry.resolve("Мой проект").unwrap().path, first);
        assert_eq!(registry.resolve("repo").unwrap().path, second);
        assert_eq!(
            registry
                .resolve(&first.to_string_lossy())
                .unwrap()
                .registered_at,
            registered_at
        );
        assert!(first.is_dir());
        assert!(!registry.has_duplicate_name(&second));
        registry.rename(&second, "Мой проект".into()).unwrap();
        assert!(registry.has_duplicate_name(&first));
        assert!(matches!(
            registry.resolve("Мой проект"),
            Err(ResolveError::Ambiguous { .. })
        ));
    }

    #[test]
    fn failed_rename_keeps_previous_name_in_memory() {
        let home = TempDir::new().unwrap();
        let project = dir(&home, "repo");
        let mut registry = Registry::load(home.path()).unwrap();
        registry.add(&project).unwrap();
        registry.file = home.path().join("missing/parent/projects.toml");
        fs::write(home.path().join("missing"), "not a directory").unwrap();
        assert!(registry.rename(&project, "Новое имя".into()).is_err());
        assert_eq!(registry.resolve("repo").unwrap().path, project);
        assert!(registry.resolve("Новое имя").is_err());
    }

    #[test]
    fn duplicate_name_needs_a_path() {
        let home = TempDir::new().unwrap();
        let first = dir(&home, "one/docs-proj");
        let second = dir(&home, "two/docs-proj");
        let mut registry = Registry::load(home.path()).unwrap();
        registry.add(&first).unwrap();
        registry.add(&second).unwrap();

        assert!(registry.has_duplicate_name(&first));
        assert!(matches!(
            registry.resolve("docs-proj"),
            Err(ResolveError::Ambiguous { .. })
        ));
        assert_eq!(
            registry.resolve(&second.to_string_lossy()).unwrap().path,
            second
        );
    }

    #[test]
    fn nested_cwd_belongs_to_the_nearest_registered_project() {
        let home = TempDir::new().unwrap();
        let outer = dir(&home, "outer");
        let inner = dir(&home, "outer/inner");
        let deep = dir(&home, "outer/inner/src/deep");
        let worktree = dir(&home, "outer/.claude/worktrees/wt");
        let mut registry = Registry::load(home.path()).unwrap();
        registry.add(&outer).unwrap();

        assert_eq!(registry.project_for_cwd(&deep).unwrap().path, outer);
        assert_eq!(registry.project_for_cwd(&worktree).unwrap().path, outer);

        registry.add(&inner).unwrap();
        assert_eq!(registry.project_for_cwd(&deep).unwrap().path, inner);
        assert!(registry.project_for_cwd(home.path()).is_none());
    }

    /// Основной checkout с `.git/worktrees/<имя>` и рабочая копия вне него.
    fn linked_worktree(home: &TempDir, main: &str, outside: &str, relative: bool) -> PathBuf {
        let admin = dir(home, &format!("{main}/.git/worktrees/wt"));
        let work = dir(home, outside);
        let target = if relative {
            "../main/.git/worktrees/wt".to_string()
        } else {
            admin.to_string_lossy().replace('\\', "/")
        };
        fs::write(work.join(".git"), format!("gitdir: {target}\n")).unwrap();
        fs::write(admin.join("commondir"), "../..\n").unwrap();
        work
    }

    #[test]
    fn worktree_outside_project_belongs_to_its_main_checkout() {
        let home = TempDir::new().unwrap();
        let work = linked_worktree(&home, "main", "runs/wt", false);
        let cwd = dir(&home, "runs/wt/src/deep");
        let main = home.path().join("main");
        let mut registry = Registry::load(home.path()).unwrap();

        assert!(registry.project_for_cwd(&cwd).is_none());
        registry.add(&main).unwrap();
        assert_eq!(registry.project_for_cwd(&work).unwrap().path, main);
        assert_eq!(registry.project_for_cwd(&cwd).unwrap().path, main);
        assert_eq!(
            registry.discover_root(&cwd),
            Discovered {
                root: main,
                registered: true
            }
        );
    }

    #[test]
    fn worktree_with_relative_gitdir_finds_its_main_checkout() {
        let home = TempDir::new().unwrap();
        let work = linked_worktree(&home, "main", "wt-rel", true);
        let main = home.path().join("main");
        let mut registry = Registry::load(home.path()).unwrap();
        registry.add(&main).unwrap();

        assert_eq!(registry.project_for_cwd(&work).unwrap().path, main);
    }

    #[test]
    fn worktree_of_unregistered_main_checkout_keeps_old_rules() {
        let home = TempDir::new().unwrap();
        let work = linked_worktree(&home, "main", "runs/wt", false);
        let registry = Registry::load(home.path()).unwrap();

        assert!(registry.project_for_cwd(&work).is_none());
        assert_eq!(
            registry.discover_root(&work),
            Discovered {
                root: work,
                registered: false
            }
        );
    }

    #[test]
    fn worktree_with_project_bars_is_not_registered_separately() {
        let home = TempDir::new().unwrap();
        let work = linked_worktree(&home, "main", "runs/wt", false);
        fs::create_dir_all(work.join(".bars")).unwrap();
        fs::write(work.join(".bars/config.toml"), "").unwrap();
        let main = home.path().join("main");
        let mut registry = Registry::load(home.path()).unwrap();
        registry.add(&main).unwrap();

        assert_eq!(
            registry.discover_root(&work),
            Discovered {
                root: main,
                registered: true
            }
        );
    }

    #[test]
    fn broken_or_foreign_git_file_is_not_a_worktree() {
        let home = TempDir::new().unwrap();
        let mut registry = Registry::load(home.path()).unwrap();
        registry.add(&dir(&home, "main")).unwrap();

        let garbage = dir(&home, "garbage");
        fs::write(garbage.join(".git"), "not a gitdir line").unwrap();
        // Подмодуль: `gitdir` ведёт в `.git/modules/<имя>`, а не в `.git/worktrees/<имя>`.
        let submodule = dir(&home, "sub");
        fs::write(submodule.join(".git"), "gitdir: ../main/.git/modules/sub").unwrap();

        assert!(registry.project_for_cwd(&garbage).is_none());
        assert!(registry.project_for_cwd(&submodule).is_none());
    }

    #[test]
    fn discover_prefers_registered_then_bars_then_git_then_cwd() {
        let home = TempDir::new().unwrap();
        let mut registry = Registry::load(home.path()).unwrap();

        let registered = dir(&home, "reg");
        registry.add(&registered).unwrap();
        let cwd = dir(&home, "reg/a/b");
        assert_eq!(
            registry.discover_root(&cwd),
            Discovered {
                root: registered,
                registered: true
            }
        );

        let with_bars = dir(&home, "bars-only");
        fs::create_dir_all(with_bars.join(".bars")).unwrap();
        fs::write(with_bars.join(".bars/config.toml"), "").unwrap();
        let cwd = dir(&home, "bars-only/x");
        assert_eq!(
            registry.discover_root(&cwd),
            Discovered {
                root: with_bars,
                registered: false
            }
        );

        // Домашняя `.bars` пользователя папку проектом не делает.
        let user_home = dir(&home, "userhome");
        fs::create_dir_all(user_home.join(".bars/models")).unwrap();
        fs::write(user_home.join(".bars/settings.toml"), "").unwrap();
        let cwd = dir(&home, "userhome/work");
        assert_eq!(registry.discover_root(&cwd).root, cwd);

        // В worktree `.git` — файл.
        let repo = dir(&home, "repo");
        fs::write(repo.join(".git"), "gitdir: elsewhere").unwrap();
        let cwd = dir(&home, "repo/src");
        assert_eq!(
            registry.discover_root(&cwd),
            Discovered {
                root: repo,
                registered: false
            }
        );

        let lone = dir(&home, "lone/work");
        assert_eq!(
            registry.discover_root(&lone),
            Discovered {
                root: lone,
                registered: false
            }
        );
    }

    #[test]
    fn has_markdown_needs_a_real_document() {
        let home = TempDir::new().unwrap();
        let project = dir(&home, "p");
        assert!(!has_markdown(&project, "docs"));

        fs::create_dir_all(project.join("docs/.hidden")).unwrap();
        fs::write(project.join("docs/.hidden/secret.md"), "x").unwrap();
        fs::write(project.join("docs/notes.txt"), "x").unwrap();
        assert!(!has_markdown(&project, "docs"));

        fs::create_dir_all(project.join("docs/guide")).unwrap();
        fs::write(project.join("docs/guide/Intro.MD"), "x").unwrap();
        assert!(has_markdown(&project, "docs"));
    }
}
