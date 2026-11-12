//! Отбор и порядок записей реестра для `projects` (Т-49, Т-50): одно правило совпадения
//! (`bars_protocol::project_matches_filter`) и порядок «предок раньше подпроектов».
//! Совпадение worktree проекта (`matched_by: worktree`, `worktrees.md`) подключается сюда же:
//! `matching` — единственное место, где решается, какие записи попали в выдачу.

use std::collections::HashSet;

use crate::worker::ProjectSnapshot;

/// Индексы записей так, что каждый предок стоит раньше своих подпроектов; корни и подпроекты
/// одного предка — в порядке реестра. Запись, чей предок не найден в списке, считается корнем.
pub(super) fn parents_first(all: &[ProjectSnapshot]) -> Vec<usize> {
    fn visit(
        all: &[ProjectSnapshot],
        index: usize,
        seen: &mut HashSet<usize>,
        order: &mut Vec<usize>,
    ) {
        if !seen.insert(index) {
            return;
        }
        order.push(index);
        for (child, snapshot) in all.iter().enumerate() {
            if snapshot.parent.as_deref() == Some(all[index].path.as_path()) {
                visit(all, child, seen, order);
            }
        }
    }
    let mut seen = HashSet::new();
    let mut order = Vec::with_capacity(all.len());
    for (index, snapshot) in all.iter().enumerate() {
        let has_parent = snapshot
            .parent
            .as_ref()
            .is_some_and(|parent| all.iter().any(|other| &other.path == parent));
        if !has_parent {
            visit(all, index, &mut seen, &mut order);
        }
    }
    // Страховка от цикла предков (в реестре его быть не должно): записи не пропадают.
    for index in 0..all.len() {
        visit(all, index, &mut seen, &mut order);
    }
    order
}

/// Индексы записей, совпавших с `text`, в порядке `parents_first`.
pub(super) fn matching(all: &[ProjectSnapshot], text: &str) -> Vec<usize> {
    parents_first(all)
        .into_iter()
        .filter(|&index| {
            let snapshot = &all[index];
            let path = snapshot.path.display().to_string();
            let mut fields = vec![snapshot.name.as_str(), path.as_str()];
            if let Some(qualified) = &snapshot.qualified_name {
                fields.push(qualified);
            }
            bars_protocol::project_matches_filter(&fields, text)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use bars_protocol::ProjectState;

    use super::*;

    fn snapshot(name: &str, path: &str, parent: Option<&str>) -> ProjectSnapshot {
        ProjectSnapshot {
            path: PathBuf::from(path),
            name: name.to_string(),
            duplicate_name: false,
            registered_at: String::new(),
            docs_root: "docs".to_string(),
            state: ProjectState::Ready,
            parent: parent.map(PathBuf::from),
            qualified_name: parent.map(|_| format!("main/{name}")),
            discovered: parent.is_some(),
            lazy: false,
            discovery_truncated: false,
        }
    }

    #[test]
    fn parent_goes_before_its_subprojects_even_when_registered_later() {
        let all = [
            snapshot("api", "C:/repos/main/api", Some("C:/repos/main")),
            snapshot("other", "C:/repos/other", None),
            snapshot("main", "C:/repos/main", None),
            snapshot("deep", "C:/repos/main/api/deep", Some("C:/repos/main/api")),
        ];
        let names: Vec<&str> = parents_first(&all)
            .into_iter()
            .map(|index| all[index].name.as_str())
            .collect();
        assert_eq!(names, ["other", "main", "api", "deep"]);
    }

    #[test]
    fn filter_keeps_a_subproject_without_its_unmatched_parent() {
        let all = [
            snapshot("main", "C:/repos/main", None),
            snapshot("web", "C:/repos/main/web", Some("C:/repos/main")),
        ];
        assert_eq!(matching(&all, "WEB"), [1]);
        assert_eq!(matching(&all, "main/web"), [1]);
        assert_eq!(matching(&all, "repos\\main"), [0, 1]);
        assert!(matching(&all, "nothing").is_empty());
    }

    #[test]
    fn filter_matches_name_qualified_name_and_path_in_unicode() {
        let all = [snapshot("Знания", "C:/Мои/Знания", None)];
        assert_eq!(matching(&all, "знан"), [0]);
        assert_eq!(matching(&all, "мои\\знания"), [0]);
    }

    #[test]
    fn a_parent_cycle_loses_no_entry() {
        let all = [
            snapshot("a", "C:/a", Some("C:/b")),
            snapshot("b", "C:/b", Some("C:/a")),
        ];
        assert_eq!(parents_first(&all).len(), 2);
    }
}
