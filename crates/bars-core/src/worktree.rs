//! Git worktree: чтение записей `.git/worktrees/*`, состояния, владелец и корень вида
//! (`docs/features/projects/worktrees.md`, «Обнаружение worktree», Т-48).
//!
//! Только чтение файлов git: команда `git` не запускается, worktree не создаются, не чинятся и не
//! удаляются (IDEA-16). Дельта индекса и поиск по виду — отдельные шаги; здесь — список worktree,
//! их состояния и привязка рабочей папки к записи.

use std::fs;
use std::path::{Component, Path, PathBuf};

/// Состояние записи `.git/worktrees/<имя>` (без учёта корня вида: он зависит от владельца).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryState {
    /// W — папка, её `.git` — файл с `gitdir:` на ту же запись.
    Live,
    /// `gitdir` пуст или ведёт в несуществующее место, либо W — не папка.
    Missing,
    /// W есть, но её `.git` не файл или ведёт в другую запись, либо запись из другого
    /// репозитория (`commondir`).
    Detached,
}

/// Одна запись `.git/worktrees/<имя>/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    /// Имя папки записи: то, что агент пишет в `wt`.
    pub name: String,
    /// Папка записи `<R>/.git/worktrees/<имя>`.
    pub admin_dir: PathBuf,
    /// Корень рабочей копии W — родитель файла из `gitdir`; `None`, если `gitdir` пуст.
    pub path: Option<PathBuf>,
    /// Ветка из `HEAD`: `refs/heads/<ветка>` даёт `<ветка>`, другой `ref:` — как есть, сырой хеш
    /// (`detached`) — `None`.
    pub branch: Option<String>,
    /// Причина блокировки git (текст файла `locked`, может быть пустой); `None` — не заблокирован.
    pub locked: Option<String>,
    pub state: EntryState,
}

/// Живой worktree владельца: проект X репозитория R, у которого корень вида существует.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedWorktree {
    pub name: String,
    pub admin_dir: PathBuf,
    /// Корень рабочей копии W.
    pub path: PathBuf,
    /// `W/<смещение владельца>`.
    pub view_root: PathBuf,
    pub branch: Option<String>,
    pub locked: Option<String>,
}

/// Репозиторий проекта: ближайшая вверх от корня проекта папка, где `.git` — каталог.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    /// Корень репозитория R (основной checkout).
    pub root: PathBuf,
    /// Путь корня проекта от R; пусто, если корень проекта и есть корень репозитория.
    pub offset: PathBuf,
}

/// Рабочая папка внутри живого worktree: запись, W и основной checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub name: String,
    pub admin_dir: PathBuf,
    /// Корень рабочей копии W.
    pub work: PathBuf,
    /// Основной checkout R.
    pub repo: PathBuf,
}

/// Репозиторий проекта. Если по пути вверх раньше каталога встретился файл `.git` (проект лежит
/// внутри worktree или подмодуля), у проекта worktree нет: `None`.
pub fn repository_of(project_root: &Path) -> Option<Repository> {
    let mut offset: Vec<std::ffi::OsString> = Vec::new();
    let mut dir = project_root;
    loop {
        let git = dir.join(".git");
        if git.is_dir() {
            return Some(Repository {
                root: dir.to_path_buf(),
                offset: offset.iter().rev().collect(),
            });
        }
        if git.is_file() {
            return None;
        }
        offset.push(dir.file_name()?.to_os_string());
        dir = dir.parent()?;
    }
}

/// Читает все записи `<repo>/.git/worktrees/*`, отсортированные по имени. Нет папки — пусто.
pub fn read_entries(repo: &Path) -> Vec<WorktreeEntry> {
    let admin_root = repo.join(".git").join("worktrees");
    let Ok(items) = fs::read_dir(&admin_root) else {
        return Vec::new();
    };
    let mut entries: Vec<WorktreeEntry> = items
        .flatten()
        .filter(|item| item.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|item| read_entry(repo, &item.path(), item.file_name().to_string_lossy().into_owned()))
        .collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

fn read_entry(repo: &Path, admin: &Path, name: String) -> WorktreeEntry {
    let branch = read_head(&admin.join("HEAD"));
    let locked = fs::read_to_string(admin.join("locked"))
        .ok()
        .map(|text| text.trim().to_string());
    let gitdir = read_text(&admin.join("gitdir"));
    let mut entry = WorktreeEntry {
        name,
        admin_dir: admin.to_path_buf(),
        path: None,
        branch,
        locked,
        state: EntryState::Missing,
    };
    let Some(text) = gitdir.filter(|text| !text.is_empty()) else {
        return entry;
    };
    let git_file = resolve(admin, &text);
    let Some(work) = git_file.parent().map(Path::to_path_buf) else {
        return entry;
    };
    entry.path = Some(work.clone());
    if !work.is_dir() {
        return entry;
    }
    entry.state = if !git_file_names(&work, admin) || !belongs_to(admin, repo) {
        EntryState::Detached
    } else {
        EntryState::Live
    };
    entry
}

/// `HEAD` записи: ветка или `None` (`detached`, нечитаемый файл).
fn read_head(path: &Path) -> Option<String> {
    let text = read_text(path)?;
    let target = text.strip_prefix("ref:")?.trim();
    if target.is_empty() {
        return None;
    }
    Some(
        target
            .strip_prefix("refs/heads/")
            .unwrap_or(target)
            .to_string(),
    )
}

/// Запись принадлежит репозиторию: `commondir` (если есть) ведёт в `<repo>/.git`, иначе запись
/// лежит там по пути.
fn belongs_to(admin: &Path, repo: &Path) -> bool {
    let common = match read_text(&admin.join("commondir")) {
        Some(text) if !text.is_empty() => resolve(admin, &text),
        _ => match admin.parent().and_then(Path::parent) {
            Some(common) => common.to_path_buf(),
            None => return false,
        },
    };
    same_path(&common, &repo.join(".git"))
}

/// Файл `.git` рабочей копии — файл с `gitdir:`, который ведёт ровно в запись `admin`.
fn git_file_names(work: &Path, admin: &Path) -> bool {
    let git = work.join(".git");
    if !git.is_file() {
        return false;
    }
    gitdir_of_file(&git).is_some_and(|target| same_path(&target, admin))
}

/// Путь из строки `gitdir: <путь>` файла `.git`; относительный путь считается от папки файла.
fn gitdir_of_file(git_file: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string(git_file).ok()?;
    let target = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("gitdir:"))?
        .trim();
    if target.is_empty() {
        return None;
    }
    Some(resolve(git_file.parent()?, target))
}

/// Ближайшая вверх от `cwd` папка с файлом `.git` — рабочая копия W, если её `gitdir:` ведёт в
/// `<R>/.git/worktrees/<имя>` и запись живая: `gitdir` записи указывает на `W/.git`, `commondir`
/// ведёт в `R/.git`. Подмодуль, bare-репозиторий, битый файл и мёртвая запись дают `None`.
pub fn locate(cwd: &Path) -> Option<Location> {
    let work = cwd.ancestors().find(|dir| dir.join(".git").is_file())?;
    let admin = gitdir_of_file(&work.join(".git"))?;
    if admin.parent()?.file_name()? != "worktrees" {
        return None;
    }
    let name = admin.file_name()?.to_string_lossy().into_owned();
    let repo = match read_text(&admin.join("commondir")) {
        Some(text) if !text.is_empty() => resolve(&admin, &text),
        _ => admin.parent()?.parent()?.to_path_buf(),
    };
    if repo.file_name()? != ".git" {
        return None;
    }
    let repo = repo.parent()?.to_path_buf();
    // Запись должна быть живой: `gitdir` записи указывает на этот же `.git`.
    let recorded = resolve(&admin, &read_text(&admin.join("gitdir"))?);
    if !same_path(&recorded, &work.join(".git")) {
        return None;
    }
    Some(Location {
        name,
        admin_dir: admin,
        work: work.to_path_buf(),
        repo,
    })
}

/// Живые worktree владельца с корнем вида в `W/<смещение>`. Записи без корня вида («без вида»),
/// пропавшие и оторванные в список не входят.
pub fn linked_for_owner(entries: &[WorktreeEntry], offset: &Path) -> Vec<LinkedWorktree> {
    entries
        .iter()
        .filter(|entry| entry.state == EntryState::Live)
        .filter_map(|entry| {
            let path = entry.path.clone()?;
            let view_root = path.join(offset);
            if !view_root.is_dir() {
                return None;
            }
            Some(LinkedWorktree {
                name: entry.name.clone(),
                admin_dir: entry.admin_dir.clone(),
                path,
                view_root,
                branch: entry.branch.clone(),
                locked: entry.locked.clone(),
            })
        })
        .collect()
}

/// Сравнение путей: слэши и (на Windows) регистр не важны.
pub fn same_path(a: &Path, b: &Path) -> bool {
    path_key(a) == path_key(b)
}

/// Ключ сравнения пути: компоненты, на Windows — в нижнем регистре.
pub fn path_key(path: &Path) -> Vec<String> {
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

/// `path` лежит внутри `dir` или совпадает с ним.
pub fn is_within(path: &Path, dir: &Path) -> bool {
    let key = path_key(path);
    let base = path_key(dir);
    key.len() >= base.len() && key[..base.len()] == base[..]
}

fn read_text(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    Some(text.trim_start_matches('\u{feff}').trim().to_string())
}

/// Путь из файла git: абсолютный как есть, относительный — от `base`; без `.` и `..`.
fn resolve(base: &Path, text: &str) -> PathBuf {
    let path = Path::new(text);
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    let mut result = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
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

    /// Репозиторий `repo` с каталогом `.git`.
    fn repo(root: &TempDir, name: &str) -> PathBuf {
        let path = dir(root, name);
        fs::create_dir_all(path.join(".git")).unwrap();
        path
    }

    /// Запись `.git/worktrees/<name>` репозитория и рабочая копия `work` с `.git`-файлом.
    fn add_worktree(repo: &Path, name: &str, work: &Path, head: &str, relative: bool) -> PathBuf {
        let admin = repo.join(".git").join("worktrees").join(name);
        fs::create_dir_all(&admin).unwrap();
        fs::create_dir_all(work).unwrap();
        fs::write(admin.join("HEAD"), head).unwrap();
        fs::write(admin.join("commondir"), "../..\n").unwrap();
        let git_file = work.join(".git");
        let gitdir = if relative {
            // Относительный путь от папки записи до `.git` рабочей копии.
            let up = Path::new("../../../..");
            let rel = work
                .strip_prefix(repo.parent().unwrap())
                .unwrap()
                .join(".git");
            up.join(rel).to_string_lossy().replace('\\', "/")
        } else {
            git_file.to_string_lossy().replace('\\', "/")
        };
        fs::write(admin.join("gitdir"), format!("{gitdir}\n")).unwrap();
        fs::write(
            &git_file,
            format!("gitdir: {}\n", admin.to_string_lossy().replace('\\', "/")),
        )
        .unwrap();
        admin
    }

    #[test]
    fn linked_worktrees_read_gitdir_head_locked() {
        let root = TempDir::new().unwrap();
        let main = repo(&root, "main");
        let first = dir(&root, "wt-branch");
        let second = dir(&root, "wt-detached");
        let third = dir(&root, "wt-other-ref");
        let admin = add_worktree(&main, "wt-branch", &first, "ref: refs/heads/feat/quorum\n", false);
        fs::write(admin.join("locked"), "на флешке\n").unwrap();
        add_worktree(&main, "wt-detached", &second, "0123456789abcdef0123456789abcdef01234567\n", true);
        add_worktree(&main, "wt-other-ref", &third, "ref: refs/remotes/origin/x\n", false);

        let entries = read_entries(&main);
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["wt-branch", "wt-detached", "wt-other-ref"]);
        assert_eq!(entries[0].branch.as_deref(), Some("feat/quorum"));
        assert_eq!(entries[0].locked.as_deref(), Some("на флешке"));
        assert_eq!(entries[0].state, EntryState::Live);
        assert!(same_path(entries[0].path.as_ref().unwrap(), &first));
        assert_eq!(entries[1].branch, None, "сырой хеш — detached");
        assert_eq!(entries[1].locked, None);
        assert_eq!(entries[1].state, EntryState::Live, "относительный gitdir");
        assert!(same_path(entries[1].path.as_ref().unwrap(), &second));
        assert_eq!(entries[2].branch.as_deref(), Some("refs/remotes/origin/x"));
    }

    #[test]
    fn dangling_gitdir_after_repo_move_is_missing() {
        let root = TempDir::new().unwrap();
        let main = repo(&root, "main");
        // Запись с `gitdir` в несуществующее место.
        let admin = main.join(".git/worktrees/api-avatar");
        fs::create_dir_all(&admin).unwrap();
        fs::write(admin.join("HEAD"), "ref: refs/heads/a\n").unwrap();
        fs::write(
            admin.join("gitdir"),
            format!("{}/gone/api-avatar/.git\n", root.path().display()).replace('\\', "/"),
        )
        .unwrap();
        // Пустой `gitdir`.
        let empty = main.join(".git/worktrees/empty");
        fs::create_dir_all(&empty).unwrap();
        fs::write(empty.join("gitdir"), "\n").unwrap();
        // W есть, а её `.git` ведёт в другую запись.
        let work = dir(&root, "moved");
        add_worktree(&main, "moved", &work, "ref: refs/heads/m\n", false);
        fs::write(
            work.join(".git"),
            format!("gitdir: {}/.git/worktrees/other\n", main.display()).replace('\\', "/"),
        )
        .unwrap();

        let entries = read_entries(&main);
        let state = |name: &str| entries.iter().find(|e| e.name == name).unwrap().state;
        assert_eq!(state("api-avatar"), EntryState::Missing);
        assert_eq!(state("empty"), EntryState::Missing);
        assert_eq!(state("moved"), EntryState::Detached);
        assert!(linked_for_owner(&entries, Path::new("")).is_empty());
        assert!(locate(&work).is_none(), "мёртвая запись не даёт вида");
    }

    #[test]
    fn locked_missing_worktree_stays_missing() {
        let root = TempDir::new().unwrap();
        let main = repo(&root, "main");
        let admin = main.join(".git/worktrees/gone");
        fs::create_dir_all(&admin).unwrap();
        fs::write(admin.join("locked"), "").unwrap();
        fs::write(admin.join("gitdir"), "/nowhere/gone/.git").unwrap();
        let entries = read_entries(&main);
        assert_eq!(entries[0].state, EntryState::Missing);
        assert_eq!(entries[0].locked.as_deref(), Some(""));
    }

    #[test]
    fn submodule_and_bare_are_not_worktrees() {
        let root = TempDir::new().unwrap();
        let main = repo(&root, "main");
        // Подмодуль: `gitdir` ведёт в `.git/modules/<имя>`.
        let sub = dir(&root, "sub");
        fs::write(sub.join(".git"), "gitdir: ../main/.git/modules/sub\n").unwrap();
        assert!(locate(&sub).is_none());
        // Битый файл.
        let garbage = dir(&root, "garbage");
        fs::write(garbage.join(".git"), "not a gitdir line").unwrap();
        assert!(locate(&garbage).is_none());
        // Bare-репозиторий: `worktrees` лежит прямо в нём, `.git`-каталога у R нет.
        let bare = dir(&root, "bare.git");
        let admin = bare.join("worktrees/w");
        fs::create_dir_all(&admin).unwrap();
        let work = dir(&root, "bare-work");
        fs::write(
            work.join(".git"),
            format!("gitdir: {}\n", admin.display()).replace('\\', "/"),
        )
        .unwrap();
        fs::write(admin.join("gitdir"), work.join(".git").to_string_lossy().replace('\\', "/")).unwrap();
        assert!(locate(&work).is_none());
        assert!(read_entries(&bare).is_empty(), "у bare нет `.git/worktrees`");
        assert!(read_entries(&main).is_empty());
    }

    #[test]
    fn locate_finds_entry_from_a_deep_folder() {
        let root = TempDir::new().unwrap();
        let main = repo(&root, "main");
        let work = dir(&root, "runs/wt");
        add_worktree(&main, "wt", &work, "ref: refs/heads/x\n", false);
        let deep = dir(&root, "runs/wt/src/deep");
        let found = locate(&deep).unwrap();
        assert_eq!(found.name, "wt");
        assert!(same_path(&found.work, &work));
        assert!(same_path(&found.repo, &main));
    }

    #[test]
    fn repository_of_stops_at_a_git_file() {
        let root = TempDir::new().unwrap();
        let main = repo(&root, "main");
        let sub = dir(&root, "main/backend/worker");
        let found = repository_of(&sub).unwrap();
        assert!(same_path(&found.root, &main));
        assert_eq!(found.offset, Path::new("backend").join("worker"));
        assert_eq!(repository_of(&main).unwrap().offset, PathBuf::new());

        // Внутри worktree `.git` — файл: репозитория (и вида) у проекта нет.
        let work = dir(&root, "wt");
        add_worktree(&main, "wt", &work, "ref: refs/heads/x\n", false);
        assert!(repository_of(&dir(&root, "wt/docs")).is_none());
    }

    #[test]
    fn view_root_needs_the_owner_offset_inside_the_worktree() {
        let root = TempDir::new().unwrap();
        let main = repo(&root, "main");
        fs::create_dir_all(main.join("backend/worker")).unwrap();
        let with = dir(&root, "wt-with");
        fs::create_dir_all(with.join("backend/worker")).unwrap();
        let without = dir(&root, "wt-without");
        add_worktree(&main, "with", &with, "ref: refs/heads/a\n", false);
        add_worktree(&main, "without", &without, "ref: refs/heads/b\n", false);

        let entries = read_entries(&main);
        let owner = linked_for_owner(&entries, Path::new("backend/worker"));
        assert_eq!(owner.len(), 1, "«без вида» не показывается");
        assert_eq!(owner[0].name, "with");
        assert!(same_path(&owner[0].view_root, &with.join("backend/worker")));
        assert_eq!(linked_for_owner(&entries, Path::new("")).len(), 2);
    }
}
