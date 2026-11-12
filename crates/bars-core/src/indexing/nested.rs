//! Вложенные проекты: область другого зарегистрированного проекта не входит в индекс внешнего
//! (`docs/features/projects/projects.md`, раздел «Вложенные проекты», Т-7, Т-15).
//!
//! Список областей считает воркер по реестру и публикует сюда ([`publish`]); обход, индекс ссылок,
//! аудиты и дерево окна читают его по корню проекта ([`areas`]). Хранилище одно на процесс: окно
//! встроено в тот же процесс, что и воркер. Чтение документов и дисковый поиск вхождений идут по диску
//! и списка не используют.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{LazyLock, PoisonError, RwLock};

/// Индексируемая область вложенного проекта внутри внешнего.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Area {
    /// Корень вложенного проекта (абсолютный путь из реестра).
    pub project: PathBuf,
    /// Папка вложенного проекта от корня внешнего через `/`.
    pub project_dir: String,
    /// Область вложенного (его `docs_root`) от корня внешнего через `/`; для корня `.` совпадает
    /// с `project_dir`.
    pub dir: String,
    /// Область — папка git worktree, а не вложенный проект; тогда `project` — корень рабочей
    /// копии W.
    pub worktree: Option<WorktreeArea>,
}

/// Чья это папка worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeArea {
    /// Имя worktree: папка записи `.git/worktrees/<имя>`.
    pub name: String,
    /// Корень проекта-владельца.
    pub owner: PathBuf,
}

/// Папка живого worktree, которую воркер нашёл при обнаружении (`docs/features/projects/worktrees.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeDir {
    pub name: String,
    pub owner: PathBuf,
    /// Корень рабочей копии W (вся папка с файлом `.git`).
    pub path: PathBuf,
}

static AREAS: LazyLock<RwLock<HashMap<String, Vec<Area>>>> = LazyLock::new(Default::default);

static WORKTREE_DIRS: LazyLock<RwLock<Vec<WorktreeDir>>> = LazyLock::new(Default::default);

/// Публикует папки живых worktree всех владельцев: следующий [`publish`] исключает их из каждого
/// проекта, чей корень документации их охватывает. `owners` — владельцы, чей список заменяется
/// целиком (папки остальных владельцев не трогаются: хранилище одно на процесс); `dirs` — новые
/// папки этих владельцев.
pub fn publish_worktrees(owners: &[PathBuf], dirs: Vec<WorktreeDir>) {
    let keys: Vec<String> = owners.iter().map(|owner| key(owner)).collect();
    let mut stored = WORKTREE_DIRS
        .write()
        .unwrap_or_else(PoisonError::into_inner);
    stored.retain(|dir| !keys.contains(&key(&dir.owner)));
    stored.extend(dirs);
}

/// Опубликованные папки worktree.
pub fn worktree_dirs() -> Vec<WorktreeDir> {
    WORKTREE_DIRS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// Области вложенных проектов, которые исключаются из внешнего `outer` с корнем документации
/// `outer_docs_root`. `projects` — все проекты реестра: корень и `docs_root` каждого.
/// Вложенными считаются все потомки любой глубины. Область, не пересекающаяся с корнем документации
/// внешнего, в список не входит: она ему не мешает.
pub fn areas_of(outer: &Path, outer_docs_root: &str, projects: &[(PathBuf, String)]) -> Vec<Area> {
    areas_with(outer, outer_docs_root, projects, &[])
}

/// То же, что [`areas_of`], плюс папки worktree внутри корня документации `outer`.
pub fn areas_with(
    outer: &Path,
    outer_docs_root: &str,
    projects: &[(PathBuf, String)],
    worktrees: &[WorktreeDir],
) -> Vec<Area> {
    let Some(docs) = join_relative(Path::new(""), outer_docs_root) else {
        return Vec::new();
    };
    let mut found: Vec<Area> = projects
        .iter()
        .filter_map(|(root, docs_root)| {
            let project_rel = inside(outer, root).filter(|rel| !rel.as_os_str().is_empty())?;
            let area_rel = join_relative(&project_rel, docs_root)?;
            // Область вне корня документации внешнего в его индекс не попадает.
            if !starts_with_folded(&area_rel, &docs) {
                return None;
            }
            Some(Area {
                project: root.clone(),
                project_dir: slashed(&project_rel),
                dir: slashed(&area_rel),
                worktree: None,
            })
        })
        .collect();
    found.extend(worktrees.iter().filter_map(|worktree| {
        let rel = inside(outer, &worktree.path).filter(|rel| !rel.as_os_str().is_empty())?;
        // Папка вне корня документации проекта ему не мешает; папка, охватывающая корень
        // документации (проект лежит внутри worktree), сюда не попадает: `rel` пуст или `None`.
        if !starts_with_folded(&rel, &docs) {
            return None;
        }
        Some(Area {
            project: worktree.path.clone(),
            project_dir: slashed(&rel),
            dir: slashed(&rel),
            worktree: Some(WorktreeArea {
                name: worktree.name.clone(),
                owner: worktree.owner.clone(),
            }),
        })
    }));
    found.sort_by(|a, b| a.dir.cmp(&b.dir));
    found
}

/// Пути областей от корня проекта: аргумент [`super::walk::walk_docs_excluding`].
pub fn prefixes(areas: &[Area]) -> Vec<String> {
    areas.iter().map(|area| area.dir.clone()).collect()
}

/// Файл (путь от корня проекта через `/`) лежит в одной из исключённых областей.
pub fn is_excluded(path: &str, excluded: &[String]) -> bool {
    excluded.iter().any(|dir| {
        let dir = dir.trim_matches('/');
        path.len() >= dir.len()
            && eq_path(&path[..dir.len()], dir)
            && (path.len() == dir.len() || path.as_bytes()[dir.len()] == b'/')
    })
}

/// Публикует области всех проектов реестра (`projects`: корень и `docs_root`). Возвращает корни
/// проектов реестра, у которых список областей изменился: им нужна проверка. Папки worktree берутся
/// из последнего [`publish_worktrees`].
pub fn publish(projects: &[(PathBuf, String)]) -> Vec<PathBuf> {
    let worktrees = worktree_dirs();
    let mut stored = AREAS.write().unwrap_or_else(PoisonError::into_inner);
    let mut changed = Vec::new();
    for (root, docs_root) in projects {
        let after = areas_with(root, docs_root, projects, &worktrees);
        let before = stored.get(&key(root)).map_or(&[][..], Vec::as_slice);
        if before != after.as_slice() {
            changed.push(root.clone());
        }
        // Записи проектов, которых в `projects` нет, не трогаются: они никому не мешают.
        stored.insert(key(root), after);
    }
    changed
}

/// Опубликованные области проекта; пусто, если проект ничего не исключает или список не публиковался.
pub fn areas(project: &Path) -> Vec<Area> {
    AREAS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key(project))
        .cloned()
        .unwrap_or_default()
}

/// Пути исключённых областей проекта от его корня.
pub fn excluded(project: &Path) -> Vec<String> {
    prefixes(&areas(project))
}

/// Путь от `outer` до `inner`, если `inner` лежит внутри `outer` (или совпадает с ним).
fn inside(outer: &Path, inner: &Path) -> Option<PathBuf> {
    let mut outer_parts = outer.components();
    let mut inner_parts = inner.components();
    loop {
        match (outer_parts.next(), inner_parts.next()) {
            (None, rest) => {
                return Some(rest.into_iter().chain(inner_parts).collect());
            }
            (Some(a), Some(b))
                if eq_path(
                    &a.as_os_str().to_string_lossy(),
                    &b.as_os_str().to_string_lossy(),
                ) => {}
            _ => return None,
        }
    }
}

/// `base` + `docs_root` без `.`; `None` для пути с `..` или абсолютного.
fn join_relative(base: &Path, docs_root: &str) -> Option<PathBuf> {
    let mut out = base.to_path_buf();
    for part in Path::new(&docs_root.replace('\\', "/")).components() {
        match part {
            Component::CurDir => {}
            Component::Normal(name) => out.push(name),
            _ => return None,
        }
    }
    Some(out)
}

fn starts_with_folded(path: &Path, prefix: &Path) -> bool {
    inside(prefix, path).is_some()
}

fn slashed(path: &Path) -> String {
    path.components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn key(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.trim_end_matches('/');
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text.to_string()
    }
}

/// Имена сравниваются без учёта регистра на Windows, где папки `Docs` и `docs` — одна папка.
pub(super) fn eq_path(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry(items: &[(&str, &str)]) -> Vec<(PathBuf, String)> {
        items
            .iter()
            .map(|(path, docs)| (PathBuf::from(path), (*docs).to_string()))
            .collect()
    }

    #[test]
    fn descendants_of_any_depth_are_nested_and_siblings_are_not() {
        let all = registry(&[
            ("/w/a", "."),
            ("/w/a/b", "docs"),
            ("/w/a/x/y/c", "."),
            ("/w/other", "docs"),
            ("/w/ab", "docs"),
        ]);
        let areas = areas_of(Path::new("/w/a"), ".", &all);

        let dirs: Vec<_> = areas.iter().map(|a| a.dir.as_str()).collect();
        assert_eq!(dirs, ["b/docs", "x/y/c"]);
        assert_eq!(areas[0].project_dir, "b");
    }

    #[test]
    fn only_the_indexable_area_is_excluded_not_the_whole_folder() {
        let all = registry(&[("/w/a", "."), ("/w/a/b", "docs")]);
        let areas = areas_of(Path::new("/w/a"), ".", &all);
        let excluded = prefixes(&areas);

        assert!(is_excluded("b/docs/x.md", &excluded));
        assert!(!is_excluded("b/README.md", &excluded));
        assert!(!is_excluded("b/docs2/x.md", &excluded));
    }

    #[test]
    fn area_outside_the_outer_docs_root_is_ignored() {
        let all = registry(&[("/w/a", "docs"), ("/w/a/b", "docs")]);

        assert!(areas_of(Path::new("/w/a"), "docs", &all).is_empty());
    }

    #[test]
    fn publish_reports_projects_whose_list_changed() {
        let outer = PathBuf::from("/nested-test/a");
        let mut all = registry(&[("/nested-test/a", ".")]);
        let first = publish(&all);
        assert!(first.is_empty());

        all.push((PathBuf::from("/nested-test/a/b"), "docs".into()));
        assert_eq!(publish(&all), std::slice::from_ref(&outer));
        assert_eq!(excluded(&outer), ["b/docs"]);

        all.pop();
        assert_eq!(publish(&all), std::slice::from_ref(&outer));
        assert!(excluded(&outer).is_empty());
    }

    fn worktree(name: &str, owner: &str, path: &str) -> WorktreeDir {
        WorktreeDir {
            name: name.into(),
            owner: PathBuf::from(owner),
            path: PathBuf::from(path),
        }
    }

    #[test]
    fn worktree_dirs_are_excluded_from_every_enclosing_project() {
        let all = registry(&[
            ("/wt-areas/a", "."),
            ("/wt-areas/a/backend/worker", "docs"),
            ("/wt-areas/solo", "docs"),
        ]);
        let dirs = [
            // Папка без точки рядом с подпроектом: исключается из внешнего, не только у владельца.
            worktree(
                "worker-wt",
                "/wt-areas/a/backend/worker",
                "/wt-areas/a/backend/worker-wt",
            ),
            worktree("wt-foo", "/wt-areas/a", "/wt-areas/a/wt-foo"),
            // Папка вне проекта ему не мешает.
            worktree("away", "/wt-areas/solo", "/wt-areas/elsewhere/away"),
        ];
        let areas = areas_with(Path::new("/wt-areas/a"), ".", &all, &dirs);
        let mut found: Vec<(&str, Option<&str>)> = areas
            .iter()
            .map(|area| {
                (
                    area.dir.as_str(),
                    area.worktree.as_ref().map(|item| item.name.as_str()),
                )
            })
            .collect();
        found.sort_unstable();
        assert_eq!(
            found,
            [
                ("backend/worker-wt", Some("worker-wt")),
                ("backend/worker/docs", None),
                ("wt-foo", Some("wt-foo")),
            ]
        );
        assert!(is_excluded("wt-foo/x.md", &prefixes(&areas)));
        assert!(areas_with(Path::new("/wt-areas/solo"), "docs", &all, &dirs).is_empty());
    }

    #[test]
    fn published_worktree_dirs_are_excluded_and_return_when_removed() {
        let outer = PathBuf::from("/wt-pub/a");
        let all = registry(&[("/wt-pub/a", ".")]);
        assert!(publish(&all).is_empty());

        publish_worktrees(
            std::slice::from_ref(&outer),
            vec![worktree("wt-foo", "/wt-pub/a", "/wt-pub/a/wt-foo")],
        );
        assert_eq!(publish(&all), std::slice::from_ref(&outer));
        assert_eq!(excluded(&outer), ["wt-foo"]);
        assert!(publish(&all).is_empty(), "список не менялся");

        // Запись пропала: папка возвращается проекту при ближайшей проверке.
        publish_worktrees(std::slice::from_ref(&outer), Vec::new());
        assert_eq!(publish(&all), std::slice::from_ref(&outer));
        assert!(excluded(&outer).is_empty());
    }

    #[test]
    fn an_unavailable_nested_project_stays_excluded_while_it_is_registered() {
        // Пути не проверяются на диске: пропавшая папка остаётся в реестре и в списке.
        let all = registry(&[("/gone/a", "."), ("/gone/a/missing", "docs")]);

        assert_eq!(areas_of(Path::new("/gone/a"), ".", &all).len(), 1);
    }
}
