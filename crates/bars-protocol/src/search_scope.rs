//! Общая служебная сводка области поиска для MCP и режимов с графом.
use crate::SearchOutput;
use std::fmt::Write as _;

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
