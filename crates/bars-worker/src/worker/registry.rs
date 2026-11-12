//! Реестр проектов: `~/.bars/projects.toml` и поиск корня проекта (`docs/architecture/projects.md`,
//! Т-5, Т-15).
//!
//! Реестр хранит пути, отображаемые имена и время регистрации. Что с проектом происходит дальше — пропал он
//! или вернулся, — решает воркер.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use bars_core::worktree;
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
    /// Найденный подпроект не переименовывается: его имя — путь от главного (Т-49).
    #[error(
        "подпроект найден автоматически: его имя — путь от главного проекта; чтобы дать своё имя, \
         зарегистрируйте его отдельным проектом"
    )]
    DiscoveredRename,
}

/// Запись реестра.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectEntry {
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// RFC 3339, UTC.
    pub registered_at: String,
    /// Запись создал автопоиск подпроектов, человек проект не регистрировал (Т-49).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub discovered: bool,
    /// Индекса ещё не требовали: проект не проверяется и не индексируется. Только вместе с
    /// `discovered`; при загрузке `lazy` без `discovered` сбрасывается.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub lazy: bool,
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
    /// Имя не найдено, но его первый сегмент — главный проект: ответ называет его подпроекты
    /// (`docs/features/projects/subprojects.md`, «Ошибки»).
    #[error("{message}")]
    UnknownSubproject {
        name: String,
        /// Готовый текст: имя, главный проект и до [`HINT_SUBPROJECTS`] его подпроектов.
        message: String,
    },
}

/// Сколько подпроектов главного перечисляет подсказка об ошибке имени.
pub const HINT_SUBPROJECTS: usize = 10;

fn unknown_subproject_text(name: &str, main: &str, subprojects: &[String], total: usize) -> String {
    if subprojects.is_empty() {
        return format!("проект `{name}` не зарегистрирован; у проекта `{main}` нет подпроектов");
    }
    let mut text = format!(
        "проект `{name}` не зарегистрирован; подпроекты `{main}`: {}",
        subprojects.join(", ")
    );
    if total > subprojects.len() {
        let rest = total - subprojects.len();
        text = format!("{text} и ещё {rest} (остальные — bars_status(filter=\"{main}/\"))");
    }
    text
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
        let mut projects = projects;
        for entry in &mut projects {
            // `lazy` бывает только у найденного подпроекта: запись человека проверяют сразу.
            if entry.lazy && !entry.discovered {
                log::warn!(
                    "В реестре проект {} помечен lazy без discovered: lazy сброшен",
                    entry.path.display()
                );
                entry.lazy = false;
            }
        }
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
        self.push(path, false)
    }

    /// Добавляет найденный автопоиском подпроект: `discovered` и `lazy`, файл записывается как у
    /// [`Registry::add`]. `Ok(false)` — путь уже в реестре, запись не меняется (Т-49).
    pub fn add_discovered(&mut self, path: &Path) -> Result<bool, RegistryError> {
        self.push(path, true)
    }

    fn push(&mut self, path: &Path, discovered: bool) -> Result<bool, RegistryError> {
        let path = normalize(path)?;
        if self.contains(&path) {
            return Ok(false);
        }
        self.projects.push(ProjectEntry {
            path,
            name: None,
            registered_at: rfc3339(SystemTime::now()),
            discovered,
            lazy: discovered,
        });
        if let Err(error) = self.save() {
            self.projects.pop();
            return Err(error);
        }
        Ok(true)
    }

    /// Снимает `lazy` (первое обращение). `Ok(false)` — записи нет или она уже не ленивая:
    /// файл не переписывается.
    pub fn clear_lazy(&mut self, path: &Path) -> Result<bool, RegistryError> {
        self.update(path, |entry| {
            let changed = entry.lazy;
            entry.lazy = false;
            changed
        })
    }

    /// Делает запись самостоятельной: человек зарегистрировал найденный подпроект (`discovered` и
    /// `lazy` сняты). `Ok(false)` — записи нет или она уже самостоятельная.
    pub fn set_standalone(&mut self, path: &Path) -> Result<bool, RegistryError> {
        self.update(path, |entry| {
            let changed = entry.discovered || entry.lazy;
            entry.discovered = false;
            entry.lazy = false;
            changed
        })
    }

    /// Меняет запись; `change` возвращает, изменилось ли что-то. Сбой записи откатывает запись в
    /// памяти.
    fn update(
        &mut self,
        path: &Path,
        change: impl FnOnce(&mut ProjectEntry) -> bool,
    ) -> Result<bool, RegistryError> {
        let key = path_key(path);
        let Some(position) = self
            .projects
            .iter()
            .position(|entry| path_key(&entry.path) == key)
        else {
            return Ok(false);
        };
        let previous = self.projects[position].clone();
        if !change(&mut self.projects[position]) {
            return Ok(false);
        }
        if let Err(error) = self.save() {
            self.projects[position] = previous;
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
        if self.projects[position].discovered {
            return Err(RegistryError::DiscoveredRename);
        }
        let previous = self.projects[position].name.replace(name);
        if let Err(error) = self.save() {
            self.projects[position].name = previous;
            return Err(error);
        }
        Ok(())
    }

    /// Ближайший зарегистрированный предок: папка строго выше корня проекта (Т-49). Путь
    /// сравнивается как в [`path_key`].
    pub fn parent_of(&self, path: &Path) -> Option<&ProjectEntry> {
        path.ancestors().skip(1).find_map(|dir| self.find(dir))
    }

    /// Главный проект: самый внешний зарегистрированный предок; у проекта без предка — он сам.
    /// `None` — ни пути, ни предка в реестре нет.
    pub fn main_of(&self, path: &Path) -> Option<&ProjectEntry> {
        path.ancestors()
            .skip(1)
            .filter_map(|dir| self.find(dir))
            .last()
            .or_else(|| self.find(path))
    }

    /// Составное имя `<имя главного>/<путь от корня главного>`; `None` у проекта без предка.
    pub fn qualified_name(&self, path: &Path) -> Option<String> {
        self.parent_of(path)?;
        let main = self.main_of(path)?;
        let skip = main.path.components().count();
        let rest: Vec<String> = path
            .components()
            .skip(skip)
            .filter_map(|component| match component {
                Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect();
        Some(format!("{}/{}", main.display_name(), rest.join("/")))
    }

    /// У проекта есть своё имя-селектор: проект без предка или самостоятельный подпроект. Найденный
    /// подпроект выбирается только составным именем.
    fn has_selector_name(&self, entry: &ProjectEntry) -> bool {
        !entry.discovered || self.parent_of(&entry.path).is_none()
    }

    /// Проект по пути, составному имени `главный/путь` или отображаемому имени. Путь точнее имени
    /// и выигрывает. В составном имени `\` равен `/`, хвостовой `/` отбрасывается.
    pub fn resolve(&self, selector: &str) -> Result<&ProjectEntry, ResolveError> {
        let as_path = Path::new(selector);
        if as_path.is_absolute()
            && let Some(entry) = self.find(as_path)
        {
            return Ok(entry);
        }
        let key = selector_key(selector);
        let named: Vec<&ProjectEntry> = self
            .projects
            .iter()
            .filter(|entry| {
                (self.has_selector_name(entry) && entry.display_name() == selector)
                    || self
                        .qualified_name(&entry.path)
                        .is_some_and(|name| name == key)
            })
            .collect();
        match named.as_slice() {
            [] => Err(self
                .unknown_subproject(&key)
                .unwrap_or_else(|| ResolveError::NotFound(selector.to_string()))),
            [entry] => Ok(entry),
            _ => Err(ResolveError::Ambiguous {
                name: selector.to_string(),
                paths: named.iter().map(|entry| entry.path.clone()).collect(),
            }),
        }
    }

    /// Первый сегмент имени — главный проект: ошибка с перечнем его подпроектов.
    fn unknown_subproject(&self, key: &str) -> Option<ResolveError> {
        let (first, _) = key.split_once('/')?;
        let main = self
            .projects
            .iter()
            .find(|entry| self.parent_of(&entry.path).is_none() && entry.display_name() == first)?;
        let prefix = format!("{}/", main.display_name());
        let mut all: Vec<String> = self
            .projects
            .iter()
            .filter(|entry| {
                self.main_of(&entry.path)
                    .is_some_and(|found| path_key(&found.path) == path_key(&main.path))
            })
            .filter_map(|entry| self.qualified_name(&entry.path))
            .filter(|name| name.starts_with(&prefix))
            .collect();
        all.sort();
        let total = all.len();
        all.truncate(HINT_SUBPROJECTS);
        Some(ResolveError::UnknownSubproject {
            name: key.to_string(),
            message: unknown_subproject_text(key, &main.display_name(), &all, total),
        })
    }

    /// Имя-селектор носят несколько проектов: окно и `projects` показывают такие с путём.
    /// Повторы считаются только среди имён-селекторов (Т-49).
    pub fn has_duplicate_name(&self, path: &Path) -> bool {
        let Some(entry) = self.find(path) else {
            return false;
        };
        if !self.has_selector_name(entry) {
            return false;
        }
        let name = entry.display_name();
        self.projects
            .iter()
            .filter(|other| self.has_selector_name(other) && other.display_name() == name)
            .count()
            > 1
    }

    /// Куда указывает `cwd`: проект и, если `cwd` лежит в корне вида живого worktree, этот
    /// worktree. Порядок (`docs/features/projects/worktrees.md`, «Изменения в реестре воркера»):
    /// (1) ближайшая вверх папка с файлом `.git` — рабочая копия W, если запись по её `gitdir:`
    /// живая; (2) владелец — зарегистрированный проект X, для которого основной checkout R — его
    /// репозиторий и `cwd` лежит в корне вида `W/<смещение X>` (при нескольких побеждает ближайший
    /// к `cwd`); (3) нет владельца или запись мертва — ближайший зарегистрированный проект вверх.
    pub fn resolve_cwd(&self, cwd: &Path) -> Option<CwdTarget<'_>> {
        if let Some(location) = worktree::locate(cwd)
            && let Some(found) = self.owner_in_view(&location, cwd)
        {
            return Some(found);
        }
        cwd.ancestors()
            .find_map(|dir| self.find(dir))
            .map(|project| CwdTarget {
                project,
                worktree: None,
            })
    }

    /// Владелец рабочей копии `location`, чей корень вида содержит `cwd`.
    fn owner_in_view(&self, location: &worktree::Location, cwd: &Path) -> Option<CwdTarget<'_>> {
        self.projects
            .iter()
            .filter(|entry| worktree::is_within(&entry.path, &location.repo))
            .filter_map(|entry| {
                let repository = worktree::repository_of(&entry.path)?;
                if !worktree::same_path(&repository.root, &location.repo) {
                    return None;
                }
                let view_root = location.work.join(&repository.offset);
                worktree::is_within(cwd, &view_root).then_some((entry, view_root))
            })
            .max_by_key(|(_, view_root)| view_root.components().count())
            .map(|(entry, view_root)| CwdTarget {
                project: entry,
                worktree: Some(WorktreeRef {
                    name: location.name.clone(),
                    admin_dir: location.admin_dir.clone(),
                    path: location.work.clone(),
                    view_root,
                    repo: location.repo.clone(),
                }),
            })
    }

    /// Проект `cwd` без сведений о worktree: оболочка над [`Registry::resolve_cwd`].
    pub fn project_for_cwd(&self, cwd: &Path) -> Option<&ProjectEntry> {
        self.resolve_cwd(cwd).map(|target| target.project)
    }

    /// Корень для хука начала сессии. Рабочая копия живого worktree даёт владельца как
    /// зарегистрированный проект с признаком worktree; хук не регистрирует ни копию, ни основной
    /// checkout, если тот не в реестре (OQ-307): тогда `None` и реестр не меняется.
    pub fn discover_root(&self, cwd: &Path) -> Option<Discovered> {
        if let Some(target) = self.resolve_cwd(cwd) {
            return Some(Discovered {
                root: target.project.path.clone(),
                registered: true,
                worktree: target.worktree,
            });
        }
        if worktree::locate(cwd).is_some() {
            return None;
        }
        let found = |root: &Path| {
            Some(Discovered {
                root: root.to_path_buf(),
                registered: false,
                worktree: None,
            })
        };
        if let Some(dir) = cwd.ancestors().find(|dir| has_project_bars(dir)) {
            return found(dir);
        }
        found(
            cwd.ancestors()
                .find(|dir| dir.join(".git").exists())
                .unwrap_or(cwd),
        )
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
    /// `cwd` лежит в корне вида живого worktree этого проекта.
    pub worktree: Option<WorktreeRef>,
}

/// Живой worktree, в котором лежит `cwd` (`docs/features/projects/worktrees.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeRef {
    /// Имя записи `.git/worktrees/<имя>`.
    pub name: String,
    pub admin_dir: PathBuf,
    /// Корень рабочей копии W.
    pub path: PathBuf,
    /// `W/<смещение владельца>`.
    pub view_root: PathBuf,
    /// Основной checkout (корень репозитория R).
    pub repo: PathBuf,
}

/// Результат [`Registry::resolve_cwd`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CwdTarget<'a> {
    pub project: &'a ProjectEntry,
    pub worktree: Option<WorktreeRef>,
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

/// Селектор как составное имя: `\` равен `/`, хвостовые `/` отбрасываются.
fn selector_key(selector: &str) -> String {
    selector
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string()
}

/// Ключ сравнения путей: на Windows регистр букв не различается.
pub(super) fn path_key(path: &Path) -> Vec<String> {
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

    /// Запись `.git/worktrees/<name>` репозитория `repo` и рабочая копия `outside` с `.git`-файлом.
    fn attach(home: &TempDir, repo: &str, name: &str, outside: &str, relative: bool) -> PathBuf {
        let admin = dir(home, &format!("{repo}/.git/worktrees/{name}"));
        let work = dir(home, outside);
        let target = if relative {
            format!("../{repo}/.git/worktrees/{name}")
        } else {
            admin.to_string_lossy().replace('\\', "/")
        };
        fs::write(work.join(".git"), format!("gitdir: {target}\n")).unwrap();
        fs::write(admin.join("commondir"), "../..\n").unwrap();
        fs::write(admin.join("HEAD"), "ref: refs/heads/feat\n").unwrap();
        fs::write(
            admin.join("gitdir"),
            work.join(".git").to_string_lossy().replace('\\', "/"),
        )
        .unwrap();
        work
    }

    /// Основной checkout `main` с одной записью `wt` и рабочая копия `outside`.
    fn linked_worktree(home: &TempDir, main: &str, outside: &str, relative: bool) -> PathBuf {
        attach(home, main, "wt", outside, relative)
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
        let target = registry.resolve_cwd(&cwd).unwrap();
        assert_eq!(target.project.path, main);
        let view = target.worktree.unwrap();
        assert_eq!(view.name, "wt");
        assert!(worktree::same_path(&view.path, &work));
        assert!(worktree::same_path(&view.view_root, &work));
        let found = registry.discover_root(&cwd).unwrap();
        assert!(found.registered);
        assert_eq!(found.root, main);
        assert_eq!(found.worktree.unwrap().name, "wt");
    }

    #[test]
    fn worktree_with_relative_gitdir_finds_its_main_checkout() {
        let home = TempDir::new().unwrap();
        let work = linked_worktree(&home, "main", "wt-rel", true);
        let main = home.path().join("main");
        let mut registry = Registry::load(home.path()).unwrap();
        registry.add(&main).unwrap();

        assert_eq!(registry.project_for_cwd(&work).unwrap().path, main);
        assert!(registry.resolve_cwd(&work).unwrap().worktree.is_some());
    }

    #[test]
    fn hook_does_not_register_worktree_or_unregistered_main() {
        let home = TempDir::new().unwrap();
        let work = linked_worktree(&home, "main", "runs/wt", false);
        fs::create_dir_all(work.join(".bars")).unwrap();
        fs::write(work.join(".bars/config.toml"), "").unwrap();
        let registry = Registry::load(home.path()).unwrap();

        assert!(registry.project_for_cwd(&work).is_none());
        assert!(
            registry.discover_root(&work).is_none(),
            "ни копия (даже со своим `.bars`), ни основной checkout не регистрируются"
        );
        assert!(registry.entries().is_empty());
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

        let found = registry.discover_root(&work).unwrap();
        assert_eq!(found.root, main);
        assert!(found.registered && found.worktree.is_some());
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
        assert!(registry.resolve_cwd(&submodule).is_none());
    }

    #[test]
    fn dead_record_gives_the_plain_nearest_project_without_a_view() {
        let home = TempDir::new().unwrap();
        // Рабочая копия внутри проекта, а запись указывает на другой `.git`.
        let work = linked_worktree(&home, "main", "main/.worktree/gone", false);
        let admin = home.path().join("main/.git/worktrees/wt");
        fs::write(admin.join("gitdir"), "/elsewhere/.git\n").unwrap();
        let main = home.path().join("main");
        let mut registry = Registry::load(home.path()).unwrap();
        registry.add(&main).unwrap();

        let target = registry.resolve_cwd(&work).unwrap();
        assert_eq!(target.project.path, main);
        assert!(target.worktree.is_none(), "мёртвая запись — не worktree");
    }

    #[test]
    fn cwd_in_worktree_resolves_owner_and_view() {
        let home = TempDir::new().unwrap();
        let main = home.path().join("main");
        let inside = attach(&home, "main", "a", "main/.worktree/a", false);
        let claude = attach(&home, "main", "b", "main/.claude/worktrees/b", false);
        let outside = attach(&home, "main", "c", "iterion/runs/c", false);
        let mut registry = Registry::load(home.path()).unwrap();
        registry.add(&main).unwrap();

        for (work, name) in [(&inside, "a"), (&claude, "b"), (&outside, "c")] {
            let cwd = work.join("docs");
            fs::create_dir_all(&cwd).unwrap();
            let target = registry.resolve_cwd(&cwd).unwrap();
            assert_eq!(target.project.path, main, "{name}");
            assert_eq!(target.worktree.as_ref().unwrap().name, name);
        }
        // Основной checkout — обычный проект без вида.
        let plain = registry.resolve_cwd(&main).unwrap();
        assert!(plain.worktree.is_none());
    }

    #[test]
    fn worktree_of_subproject_belongs_to_it() {
        let home = TempDir::new().unwrap();
        let outer = dir(&home, "tmc");
        fs::create_dir_all(outer.join(".git")).unwrap();
        let worker = dir(&home, "tmc/backend/worker");
        let work = attach(
            &home,
            "tmc/backend/worker",
            "worker-wt-quorum",
            "tmc/backend/worker-wt-quorum",
            false,
        );
        let mut registry = Registry::load(home.path()).unwrap();
        registry.add(&outer).unwrap();
        registry.add_discovered(&worker).unwrap();

        let cwd = dir(&home, "tmc/backend/worker-wt-quorum/src");
        let target = registry.resolve_cwd(&cwd).unwrap();
        assert_eq!(
            target.project.path, worker,
            "владелец — подпроект с `.git`-каталогом"
        );
        assert!(worktree::same_path(&target.worktree.unwrap().path, &work));
    }

    #[test]
    fn owner_with_an_offset_needs_its_folder_in_the_view() {
        let home = TempDir::new().unwrap();
        let repo = dir(&home, "mono");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let service = dir(&home, "mono/services/api");
        let work = attach(&home, "mono", "wt", "runs/mono-wt", false);
        let mut registry = Registry::load(home.path()).unwrap();
        registry.add(&service).unwrap();

        // В ветке worktree папки проекта нет: `cwd` в W вне корня вида — не вид.
        let cwd = dir(&home, "runs/mono-wt/other");
        assert!(registry.resolve_cwd(&cwd).is_none());
        // Появилась папка проекта: вид есть, корень вида — `W/services/api`.
        let inner = dir(&home, "runs/mono-wt/services/api/docs");
        let target = registry.resolve_cwd(&inner).unwrap();
        assert_eq!(target.project.path, service);
        assert!(worktree::same_path(
            &target.worktree.unwrap().view_root,
            &work.join("services/api")
        ));
    }

    #[test]
    fn discover_prefers_registered_then_bars_then_git_then_cwd() {
        let home = TempDir::new().unwrap();
        let mut registry = Registry::load(home.path()).unwrap();

        let registered = dir(&home, "reg");
        registry.add(&registered).unwrap();
        let cwd = dir(&home, "reg/a/b");
        let found = registry.discover_root(&cwd).unwrap();
        assert_eq!((found.root, found.registered), (registered, true));
        assert!(found.worktree.is_none());

        let with_bars = dir(&home, "bars-only");
        fs::create_dir_all(with_bars.join(".bars")).unwrap();
        fs::write(with_bars.join(".bars/config.toml"), "").unwrap();
        let cwd = dir(&home, "bars-only/x");
        let found = registry.discover_root(&cwd).unwrap();
        assert_eq!((found.root, found.registered), (with_bars, false));

        // Домашняя `.bars` пользователя папку проектом не делает.
        let user_home = dir(&home, "userhome");
        fs::create_dir_all(user_home.join(".bars/models")).unwrap();
        fs::write(user_home.join(".bars/settings.toml"), "").unwrap();
        let cwd = dir(&home, "userhome/work");
        assert_eq!(registry.discover_root(&cwd).unwrap().root, cwd);

        // В worktree `.git` — файл.
        let repo = dir(&home, "repo");
        fs::write(repo.join(".git"), "gitdir: elsewhere").unwrap();
        let cwd = dir(&home, "repo/src");
        let found = registry.discover_root(&cwd).unwrap();
        assert_eq!((found.root, found.registered), (repo, false));

        let lone = dir(&home, "lone/work");
        let found = registry.discover_root(&lone).unwrap();
        assert_eq!((found.root, found.registered), (lone, false));
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

    /// Реестр по образцу `tmc-opovechenie`: главный, найденный подпроект и самостоятельный.
    fn tree(home: &TempDir) -> (Registry, PathBuf, PathBuf, PathBuf) {
        let main = dir(home, "tmc-opovechenie");
        let api = dir(home, "tmc-opovechenie/backend/api");
        let web = dir(home, "tmc-opovechenie/frontend/web-angular");
        let mut registry = Registry::load(home.path()).unwrap();
        registry.add(&main).unwrap();
        registry.add_discovered(&api).unwrap();
        registry.add(&web).unwrap();
        (registry, main, api, web)
    }

    #[test]
    fn old_file_without_new_fields_reads_as_registered_by_a_human() {
        let home = TempDir::new().unwrap();
        let project = dir(&home, "alpha");
        let text = format!(
            "[[project]]\npath = {:?}\nregistered_at = \"2026-11-01T00:00:00Z\"\n",
            project.to_string_lossy()
        );
        fs::write(home.path().join(REGISTRY_FILE), text).unwrap();
        let registry = Registry::load(home.path()).unwrap();
        let entry = &registry.entries()[0];
        assert!(!entry.discovered && !entry.lazy);
    }

    #[test]
    fn discovered_and_lazy_roundtrip_and_false_is_not_written() {
        let home = TempDir::new().unwrap();
        let (_, main, api, _) = tree(&home);
        let text = fs::read_to_string(home.path().join(REGISTRY_FILE)).unwrap();
        assert_eq!(text.matches("discovered = true").count(), 1);
        assert_eq!(text.matches("lazy = true").count(), 1);
        assert!(!text.contains("false"), "{text}");

        let reloaded = Registry::load(home.path()).unwrap();
        let entry = reloaded.find(&api).unwrap();
        assert!(entry.discovered && entry.lazy);
        assert!(!reloaded.find(&main).unwrap().discovered);
    }

    #[test]
    fn lazy_without_discovered_is_normalized_on_load() {
        let home = TempDir::new().unwrap();
        let project = dir(&home, "alpha");
        let text = format!(
            "[[project]]\npath = {:?}\nregistered_at = \"2026-11-01T00:00:00Z\"\nlazy = true\n",
            project.to_string_lossy()
        );
        fs::write(home.path().join(REGISTRY_FILE), text).unwrap();
        let registry = Registry::load(home.path()).unwrap();
        assert!(!registry.entries()[0].lazy);
    }

    #[test]
    fn parent_main_and_qualified_name_of_any_depth() {
        let home = TempDir::new().unwrap();
        let (mut registry, main, api, web) = tree(&home);
        let deep = dir(&home, "tmc-opovechenie/backend/api/vendor/x");
        registry.add_discovered(&deep).unwrap();

        assert!(registry.parent_of(&main).is_none());
        assert_eq!(registry.parent_of(&api).unwrap().path, main);
        assert_eq!(registry.parent_of(&deep).unwrap().path, api);
        // Главный — самый внешний, а не ближайший предок.
        assert_eq!(registry.main_of(&deep).unwrap().path, main);
        assert_eq!(registry.main_of(&main).unwrap().path, main);
        assert_eq!(
            registry.qualified_name(&deep).unwrap(),
            "tmc-opovechenie/backend/api/vendor/x"
        );
        assert_eq!(
            registry.qualified_name(&web).unwrap(),
            "tmc-opovechenie/frontend/web-angular"
        );
        assert!(registry.qualified_name(&main).is_none());
    }

    #[test]
    fn qualified_name_follows_a_renamed_main() {
        let home = TempDir::new().unwrap();
        let (mut registry, main, api, _) = tree(&home);
        registry.rename(&main, "Оповещение".into()).unwrap();
        assert_eq!(
            registry.qualified_name(&api).unwrap(),
            "Оповещение/backend/api"
        );
        assert_eq!(
            registry.resolve("Оповещение/backend/api").unwrap().path,
            api
        );
        assert!(registry.resolve("tmc-opovechenie/backend/api").is_err());
    }

    #[test]
    fn resolve_accepts_backslashes_and_trailing_slash_in_a_qualified_name() {
        let home = TempDir::new().unwrap();
        let (registry, _, api, _) = tree(&home);
        for selector in [
            "tmc-opovechenie/backend/api",
            "tmc-opovechenie\\backend\\api",
            "tmc-opovechenie/backend/api/",
            "tmc-opovechenie\\backend\\api\\",
        ] {
            assert_eq!(registry.resolve(selector).unwrap().path, api, "{selector}");
        }
    }

    #[test]
    fn standalone_subproject_answers_to_both_names_discovered_only_to_the_qualified() {
        let home = TempDir::new().unwrap();
        let (registry, _, api, web) = tree(&home);
        assert_eq!(registry.resolve("web-angular").unwrap().path, web);
        assert_eq!(
            registry
                .resolve("tmc-opovechenie/frontend/web-angular")
                .unwrap()
                .path,
            web
        );
        // У найденного своего имени-селектора нет.
        assert!(matches!(
            registry.resolve("api"),
            Err(ResolveError::NotFound(_))
        ));
        assert_eq!(registry.resolve(&api.to_string_lossy()).unwrap().path, api);
    }

    #[test]
    fn two_discovered_api_in_different_mains_do_not_clash() {
        let home = TempDir::new().unwrap();
        let first = dir(&home, "one");
        let second = dir(&home, "two");
        let first_api = dir(&home, "one/api");
        let second_api = dir(&home, "two/api");
        let mut registry = Registry::load(home.path()).unwrap();
        registry.add(&first).unwrap();
        registry.add(&second).unwrap();
        registry.add_discovered(&first_api).unwrap();
        registry.add_discovered(&second_api).unwrap();

        assert_eq!(registry.resolve("one/api").unwrap().path, first_api);
        assert_eq!(registry.resolve("two/api").unwrap().path, second_api);
        assert!(!registry.has_duplicate_name(&first_api));
        assert!(!registry.has_duplicate_name(&second_api));
    }

    #[test]
    fn duplicate_names_count_selector_names_only() {
        let home = TempDir::new().unwrap();
        let main = dir(&home, "main");
        let found = dir(&home, "main/web");
        let top = dir(&home, "other/web");
        let mut registry = Registry::load(home.path()).unwrap();
        registry.add(&main).unwrap();
        registry.add_discovered(&found).unwrap();
        registry.add(&top).unwrap();
        // Найденный `web` имени-селектора не имеет: самостоятельный `web` не дубликат.
        assert!(!registry.has_duplicate_name(&top));
        assert!(!registry.has_duplicate_name(&found));
        assert_eq!(registry.resolve("web").unwrap().path, top);
    }

    #[test]
    fn rename_of_a_discovered_subproject_is_refused() {
        let home = TempDir::new().unwrap();
        let (mut registry, _, api, web) = tree(&home);
        assert!(matches!(
            registry.rename(&api, "x".into()),
            Err(RegistryError::DiscoveredRename)
        ));
        assert!(registry.find(&api).unwrap().name.is_none());
        registry.rename(&web, "web".into()).unwrap();
        assert_eq!(registry.resolve("web").unwrap().path, web);
    }

    #[test]
    fn human_name_with_a_slash_colliding_with_a_qualified_name_is_ambiguous() {
        let home = TempDir::new().unwrap();
        let (mut registry, _, api, _) = tree(&home);
        let other = dir(&home, "elsewhere/x");
        registry.add(&other).unwrap();
        registry
            .rename(&other, "tmc-opovechenie/backend/api".into())
            .unwrap();
        match registry.resolve("tmc-opovechenie/backend/api") {
            Err(ResolveError::Ambiguous { paths, .. }) => {
                assert_eq!(paths.len(), 2);
                assert!(paths.contains(&api) && paths.contains(&other));
            }
            outcome => panic!("ожидалась неоднозначность, получено {outcome:?}"),
        }
    }

    #[test]
    fn unknown_name_under_a_main_lists_its_subprojects() {
        let home = TempDir::new().unwrap();
        let (registry, _, _, _) = tree(&home);
        let error = registry
            .resolve("tmc-opovechenie/backend/nope")
            .unwrap_err();
        let text = error.to_string();
        assert!(matches!(error, ResolveError::UnknownSubproject { .. }));
        assert!(text.contains("tmc-opovechenie/backend/api"), "{text}");
        assert!(
            text.contains("tmc-opovechenie/frontend/web-angular"),
            "{text}"
        );
        // Префикса нет среди главных: прежняя ошибка.
        assert!(matches!(
            registry.resolve("nobody/backend/api"),
            Err(ResolveError::NotFound(_))
        ));
    }

    #[test]
    fn subproject_hint_is_capped() {
        let home = TempDir::new().unwrap();
        let main = dir(&home, "main");
        let mut registry = Registry::load(home.path()).unwrap();
        registry.add(&main).unwrap();
        for index in 0..12 {
            registry
                .add_discovered(&dir(&home, &format!("main/s{index:02}")))
                .unwrap();
        }
        let text = registry.resolve("main/zzz").unwrap_err().to_string();
        assert!(
            text.contains("main/s09") && !text.contains("main/s10"),
            "{text}"
        );
        assert!(text.contains("и ещё 2"), "{text}");
    }

    #[test]
    fn clear_lazy_and_set_standalone_change_only_what_they_name() {
        let home = TempDir::new().unwrap();
        let (mut registry, _, api, web) = tree(&home);
        assert!(registry.clear_lazy(&api).unwrap());
        assert!(!registry.clear_lazy(&api).unwrap(), "повтор не пишет файл");
        assert!(!registry.find(&api).unwrap().lazy);
        assert!(registry.find(&api).unwrap().discovered);
        assert!(!registry.clear_lazy(&home.path().join("none")).unwrap());

        assert!(registry.set_standalone(&api).unwrap());
        assert!(!registry.set_standalone(&api).unwrap());
        assert!(!registry.set_standalone(&web).unwrap());
        let reloaded = Registry::load(home.path()).unwrap();
        let entry = reloaded.find(&api).unwrap();
        assert!(!entry.discovered && !entry.lazy);
        // Самостоятельным подпроект получает своё имя-селектор.
        assert_eq!(reloaded.resolve("api").unwrap().path, api);
    }
}
