//! Снимок до Parameters: missing, null и явное значение остаются различимыми.
use rmcp::handler::server::{common::FromContextPart, tool::ToolCallContext};
use serde_json::Value;

/// Основные аргументы графовых операций (ISS-31); тот же список проверяет история воркера.
const GRAPH: &[&str] = &[
    "format",
    "project",
    "query",
    "entity_ref",
    "from",
    "to",
    "roots",
    "documents",
    "modules",
    "files",
    "topic",
    "whole_scope",
    "analysis",
    "view",
    "local",
    "budgets",
    "limit",
    "roles",
    "task_profile",
];

pub struct Original(pub Value);
impl<S> FromContextPart<ToolCallContext<'_, S>> for Original {
    fn from_context_part(context: &mut ToolCallContext<S>) -> Result<Self, rmcp::ErrorData> {
        let allowed: &[&str] = match context.name.as_ref() {
            "bars_search" => &[
                "project",
                "query",
                "like",
                "occurrences",
                "in",
                "limit",
                "cursor",
                "detail",
                "diagnostic",
                "mode",
                "graph_budgets",
                "include_related",
                "related_budget",
            ],
            "bars_relations" | "bars_paths" | "bars_impact" | "bars_context" | "bars_explore"
            | "bars_graph" => GRAPH,
            "bars_read" => &["project", "path", "lines", "under", "view", "direction", "cursor"],
            "bars_status" => &["project", "all", "refresh"],
            "bars_audit" => &[
                "project", "kind", "action", "id", "revision", "in", "cursor", "limit", "show",
            ],
            _ => &[],
        };
        let mut fields: serde_json::Map<String, Value> = context
            .arguments
            .as_ref()
            .map(|o| {
                o.iter()
                    .filter(|(k, _)| allowed.contains(&k.as_str()))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            })
            .unwrap_or_default();
        if context.name.as_ref() == "bars_read" {
            fields.insert(
                "cursor_present".into(),
                Value::Bool(
                    context
                        .arguments
                        .as_ref()
                        .and_then(|a| a.get("cursor"))
                        .is_some_and(|v| !v.is_null()),
                ),
            );
        }
        Ok(Self(Value::Object(fields)))
    }
}
