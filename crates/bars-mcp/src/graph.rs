use super::*;
use bars_graph::{
    Analysis, Budgets, Direction, Endpoint as GraphEndpoint, GraphRequest, ObjectRef, Operation,
    PathKind, Polarity, Predicate, Role, Scope, View,
};
use std::collections::BTreeMap;

#[derive(Debug, Default, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct GraphArgs {
    pub project: Option<String>,
    pub format: Option<Format>,
    /// Вопрос на языке пользователя; обязателен для context и один из входов relations.
    pub query: Option<String>,
    pub entity_ref: Option<ObjectRef>,
    pub from: Option<GraphEndpoint>,
    pub to: Option<GraphEndpoint>,
    pub roots: Vec<ObjectRef>,
    /// Область как у rag_search: существующие модули и/или файлы от docs-root.
    pub modules: Vec<String>,
    pub files: Vec<String>,
    pub snapshot: Option<String>,
    pub cursor: Option<String>,
    pub budgets: Budgets,
    pub limit: Option<u32>,
    pub predicates: Vec<Predicate>,
    pub direction: Direction,
    pub polarity: Option<Polarity>,
    pub qualifiers: BTreeMap<String, String>,
    pub view: View,
    pub local: bool,
    pub include_paths: bool,
    pub path_kind: PathKind,
    pub max_hops: Option<u32>,
    pub max_paths: Option<u32>,
    pub topic: Option<String>,
    pub whole_scope: bool,
    pub analysis: Analysis,
    pub grouping: bars_graph::Grouping,
    pub roles: Vec<Role>,
    pub task_profile: Option<bars_graph::TaskProfile>,
    pub token_budget: Option<u32>,
    pub tokenizer_id: Option<String>,
    pub change_description: Option<String>,
}
impl BarsMcp {
    pub(super) async fn graph_call(&self, args: GraphArgs, operation: Operation) -> CallToolResult {
        let format = args.format;
        let target = self.target(args.project);
        let request = GraphRequest {
            operation,
            scope: Scope {
                modules: args.modules,
                files: args.files,
            },
            budgets: args.budgets,
            limit: args.limit,
            snapshot: args.snapshot,
            cursor: args.cursor,
            query: args.query,
            entity_ref: args.entity_ref,
            from: args.from,
            to: args.to,
            roots: args.roots,
            predicates: args.predicates,
            direction: args.direction,
            polarity: args.polarity,
            qualifiers: args.qualifiers,
            view: args.view,
            local: args.local,
            include_paths: args.include_paths,
            path_kind: args.path_kind,
            max_hops: args.max_hops,
            max_paths: args.max_paths,
            topic: args.topic,
            whole_scope: args.whole_scope,
            analysis: args.analysis,
            grouping: args.grouping,
            roles: args.roles,
            task_profile: args.task_profile,
            token_budget: args.token_budget,
            tokenizer_id: args.tokenizer_id,
            change_description: args.change_description,
        };
        finish(
            self.call(&bars_protocol::GraphQueryParams { target, request })
                .await,
            format,
            |out| {
                format!(
                    "=== GRAPH: {:?} ===\n{}\n=== END GRAPH ===",
                    out.operation,
                    json(out)
                )
            },
        )
    }
}
