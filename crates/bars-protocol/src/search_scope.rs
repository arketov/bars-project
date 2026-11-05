//! Общая служебная сводка области поиска для MCP и режимов с графом.
use crate::SearchOutput;
use std::fmt::Write as _;

/// Строка для агента об исключённых вложенных проектах: «исключены вложенные проекты: B (ищите
/// project=B)». Содержание по-русски, рамка вокруг неё — служебная (OQ-256). `None`, если исключений нет.
pub fn excluded_projects_line(nested: &[crate::NestedProjectInfo]) -> Option<String> {
    if nested.is_empty() {
        return None;
    }
    let items: Vec<String> = nested
        .iter()
        .map(|item| {
            // Одинаковые имена различаются путём: параметр `project` с таким именем неоднозначен.
            let selector = if nested
                .iter()
                .filter(|other| other.name == item.name)
                .count()
                > 1
            {
                &item.path
            } else {
                &item.name
            };
            format!("{} (ищите project={selector})", item.name)
        })
        .collect();
    Some(format!("исключены вложенные проекты: {}", items.join(", ")))
}

pub fn search_scope_text(output: &SearchOutput, primary_search: bool) -> String {
    let mut text = String::new();
    let scope = &output.applied.scope;
    let mut filters = Vec::new();
    if !scope.modules.is_empty() {
        filters.push(format!("modules {}", scope.modules.join(", ")));
    }
    if !scope.files.is_empty() {
        filters.push(format!("files {}", scope.files.join(", ")));
    }
    let whole = filters.is_empty();
    let _ = writeln!(
        text,
        "scope: {}",
        if whole {
            "whole project".into()
        } else {
            filters.join("; ")
        }
    );
    let counts = |map: &std::collections::BTreeMap<String, u32>| {
        if map.is_empty() {
            "none".into()
        } else {
            map.iter()
                .map(|(name, n)| format!("{name} {n}"))
                .collect::<Vec<_>>()
                .join(", ")
        }
    };
    if let Some(line) = excluded_projects_line(&scope.excluded_projects) {
        let _ = writeln!(text, "{line}");
    }
    let _ = writeln!(
        text,
        "by_module: candidates {}; results {}",
        counts(&output.by_module.candidates),
        counts(&output.by_module.results)
    );
    if whole && output.by_module.results.len() > 1 {
        let _ = writeln!(
            text,
            "Results span several modules; pass modules to narrow the scope."
        );
    }
    if primary_search && output.results.is_empty() && !scope.modules.is_empty() {
        let _ = writeln!(
            text,
            "No results in the selected modules; try the whole project. Zero results do not prove absence."
        );
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NestedProjectInfo;

    fn nested(name: &str, path: &str) -> NestedProjectInfo {
        NestedProjectInfo {
            name: name.into(),
            path: path.into(),
            dir: name.into(),
            area: format!("{name}/docs"),
            documents: 0,
        }
    }

    #[test]
    fn line_names_excluded_projects_and_how_to_select_them() {
        assert_eq!(excluded_projects_line(&[]), None);
        assert_eq!(
            excluded_projects_line(&[nested("B", "/a/B"), nested("C", "/a/C")]).unwrap(),
            "исключены вложенные проекты: B (ищите project=B), C (ищите project=C)"
        );
    }

    #[test]
    fn equal_names_are_selected_by_path() {
        let line = excluded_projects_line(&[nested("B", "/a/x/B"), nested("B", "/a/y/B")]).unwrap();

        assert!(line.contains("project=/a/x/B") && line.contains("project=/a/y/B"));
    }
}
