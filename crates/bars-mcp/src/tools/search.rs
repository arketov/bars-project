//! `bars_search` — один поиск, три вида запроса: вопрос (гибридный поиск по индексу),
//! `like` (похожие разделы других документов и входящие ссылки) и `occurrences` (все вхождения
//! строки на диске). Область — произвольные пути-префиксы `in` вместо модулей и файлов.

use super::*;
use serde_json::Value;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchArgs {
    /// The task question in the user's language, after the subject is clear — not a template like
    /// "step N requirements error". Operators: "exact phrase" (words in a row, required), -word
    /// and -"phrase" (exclude). A query of only exclusions is an error.
    pub query: Option<String>,
    /// Similar sections of other documents plus documents linking to the source: "path:lines"
    /// from a result line (docs/a.md:12-30) or a chunk id. Replaces a manual similar+links pair.
    pub like: Option<String>,
    /// true — find every occurrence of one exact string in saved .md files on disk (a rename, a
    /// complete list, text not yet indexed). query is the string; /pattern/ searches a regex.
    #[serde(default)]
    pub occurrences: bool,
    /// Scope: path prefixes such as features, features/search, adr/0007-one-search-scores.md.
    /// Accepted from the documentation root and from the project root, as printed in results;
    /// nested folders work like top-level ones; several prefixes are unioned. Empty — whole
    /// project. A path with no documents is an error listing the nearest folders.
    #[serde(default)]
    pub r#in: Vec<String>,
    /// How many results to return (occurrences: how many documents, then continue with cursor).
    pub limit: Option<u32>,
    /// Opaque continuation from the previous answer (occurrences pages).
    pub cursor: Option<String>,
    /// Result volume: auto (default, first results in full, the rest as lines and excerpts),
    /// full — every result in full, brief — result lines only.
    pub detail: Option<Detail>,
    /// Service check: usual results, demand is not recorded in New Topics.
    #[serde(default)]
    pub diagnostic: bool,
    pub project: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GraphSearchArgs {
    /// The task question in the user's language. Operators: "exact phrase" (required), -word
    /// (exclude). Graph modes keep the same hybrid scoring.
    pub query: Option<String>,
    /// Similar sections of other documents plus documents linking to the source (see the base
    /// bars_search description).
    pub like: Option<String>,
    /// true — every occurrence of one exact string on disk (see the base bars_search description).
    #[serde(default)]
    pub occurrences: bool,
    /// Scope: path prefixes (see the base bars_search description).
    #[serde(default)]
    pub r#in: Vec<String>,
    /// Result volume: auto (default), full or brief.
    pub detail: Option<Detail>,
    /// Opaque continuation from the previous answer (occurrences pages).
    pub cursor: Option<String>,
    /// Service check: usual results, demand is not recorded in New Topics.
    #[serde(default)]
    pub diagnostic: bool,
    pub project: Option<String>,
    /// hybrid — plain search; mix — plus related sources from the graph (voluminous, set
    /// related_budget); graph — relations instead of fragments. Without mode — the same three
    /// queries as the base bars_search.
    pub mode: Option<bars_graph::RetrievalMode>,
    /// Only for mix and graph: graph traversal budgets.
    #[serde(default)]
    pub graph_budgets: bars_graph::Budgets,
    /// Only for mix: false — without related sources; default true.
    pub include_related: Option<bool>,
    /// Only for mix: byte budget of the related package.
    pub related_budget: Option<u32>,
    pub limit: Option<u32>,
}

impl BarsMcp {
    async fn search_impl(&self, args: SearchArgs, original: Value) -> CallToolResult {
        if args.like.is_some() && (args.query.is_some() || args.occurrences) {
            return CallToolResult::error(vec![ContentBlock::text(
                "invalid_params: pass either like, or query (optionally with occurrences=true)",
            )]);
        }
        if args.like.is_none() && args.query.is_none() {
            return CallToolResult::error(vec![ContentBlock::text(
                "invalid_params: pass query, like, or occurrences=true with query as the string",
            )]);
        }
        let paths = match scope_paths(&args.r#in) {
            Ok(paths) => paths,
            Err(message) => {
                return CallToolResult::error(vec![ContentBlock::text(message)]);
            }
        };
        if let Some(like) = args.like.clone() {
            let params = bars_protocol::LikeParams {
                target: self.target(args.project),
                source: like,
                paths,
                limit: args.limit,
            };
            return present(
                self.call_observed(&params, Some(&original)).await,
                render::like,
            );
        }
        let Some(query) = args.query.clone() else {
            return CallToolResult::error(vec![ContentBlock::text(
                "invalid_params: occurrences=true needs query as the string",
            )]);
        };
        if args.occurrences {
            let cursor = match parse_cursor(&args.cursor) {
                Ok(cursor) => cursor,
                Err(message) => {
                    return CallToolResult::error(vec![ContentBlock::text(message)]);
                }
            };
            let (needle, regex) = match strip_regex_slashes(&query) {
                Some(pattern) => (pattern, true),
                None => (query, false),
            };
            let params = bars_protocol::DiskTextParams {
                target: self.target(args.project),
                query: needle,
                case_sensitive: false,
                regex,
                path_filter: String::new(),
                modules: Vec::new(),
                files: Vec::new(),
                paths,
                limit_docs: args.limit.map(|value| value as usize),
                limit_hits: None,
                path: cursor.path,
                offset: cursor.offset,
                revision: cursor.revision,
            };
            return present(
                self.call_observed(&params, Some(&original)).await,
                render::occurrences,
            );
        }
        let params = bars_protocol::SearchParams {
            target: self.target(args.project),
            query,
            diagnostic: args.diagnostic,
            modules: Vec::new(),
            files: Vec::new(),
            paths,
            limit: args.limit,
            candidates: None,
            min_score: None,
            wait_models: None,
        };
        let detail = args.detail.unwrap_or_default();
        present(
            self.call_observed(&params, Some(&original)).await,
            |output| render::search(output, detail),
        )
    }

    async fn graph_search_impl(&self, args: GraphSearchArgs, original: Value) -> CallToolResult {
        if args.mode.is_none() {
            // Без mode графовая форма ведёт себя как обычная.
            return self
                .search_impl(
                    SearchArgs {
                        query: args.query,
                        like: args.like,
                        occurrences: args.occurrences,
                        r#in: args.r#in,
                        limit: args.limit,
                        cursor: args.cursor,
                        detail: args.detail,
                        diagnostic: args.diagnostic,
                        project: args.project,
                    },
                    original,
                )
                .await;
        }
        let paths = match scope_paths(&args.r#in) {
            Ok(paths) => paths,
            Err(message) => {
                return CallToolResult::error(vec![ContentBlock::text(message)]);
            }
        };
        if args.like.is_some() && (args.query.is_some() || args.occurrences) {
            return CallToolResult::error(vec![ContentBlock::text(
                "invalid_params: pass either like, or query (optionally with occurrences=true)",
            )]);
        }
        if (args.include_related.is_some() || args.related_budget.is_some())
            && args.mode != Some(bars_graph::RetrievalMode::Mix)
        {
            return CallToolResult::error(vec![ContentBlock::text(
                "invalid_params: include_related/related_budget need mode=mix",
            )]);
        }
        let Some(query) = args.query.clone() else {
            return CallToolResult::error(vec![ContentBlock::text(
                "invalid_params: graph modes need query",
            )]);
        };
        let search = bars_protocol::SearchParams {
            target: self.target(args.project),
            query,
            diagnostic: args.diagnostic,
            modules: Vec::new(),
            files: Vec::new(),
            paths,
            limit: args.limit,
            candidates: None,
            min_score: None,
            wait_models: None,
        };
        let params = bars_protocol::GraphSearchParams {
            text: true,
            search,
            mode: args.mode.expect("mode проверен выше"),
            graph_budgets: args.graph_budgets,
            include_related: args.include_related.unwrap_or(true),
            related_budget: args.related_budget,
        };
        present(
            self.call_observed(&params, Some(&original)).await,
            bars_protocol::graph_text::render_graph_search_text,
        )
    }
}

/// "/pattern/" — regex-форма точного поиска; не путать с путями, оканчивающимися на `/`.
fn strip_regex_slashes(query: &str) -> Option<String> {
    let inner = query.strip_prefix('/')?.strip_suffix('/')?;
    (!inner.is_empty() && !inner.contains('/')).then(|| inner.to_string())
}

fn parse_cursor(cursor: &Option<String>) -> Result<OccurrencesCursor, &'static str> {
    let Some(cursor) = cursor else {
        return Ok(OccurrencesCursor::default());
    };
    serde_json::from_str(cursor).map_err(|_| {
        "invalid_params: cursor is not from a previous occurrences answer; start without cursor"
    })
}

pub(crate) fn search_router() -> ToolRouter<BarsMcp> {
    BarsMcp::search_tool_router()
}

pub(crate) fn graph_router() -> ToolRouter<BarsMcp> {
    BarsMcp::graph_search_tool_router()
}

#[tool_router(router = search_tool_router)]
impl BarsMcp {
    #[tool(
        name = "bars_search",
        annotations(read_only_hint = true),
        description = "Search project documentation with this instead of Grep or Glob. \
                       One tool, three queries. (1) Question: pass the task question in the \
                       user's language as query; results are scored sections with sources and \
                       lines. (2) Similar: like takes \"path:lines\" from a result line or a \
                       chunk id and returns similar sections of OTHER documents plus documents \
                       linking to the source. (3) Occurrences: occurrences=true finds every \
                       occurrence of one exact string on disk (query is the string, /pattern/ \
                       for regex), paginated by cursor. Narrow the scope with in: path prefixes \
                       (features, features/search, adr/0007.md); nested folders work, several \
                       prefixes union; unclear kind of knowledge — search the whole project and \
                       widen after weak results. Zero results do not prove absence: check \
                       subject, project, scope."
    )]
    async fn bars_search(
        &self,
        original: observation::Original,
        Parameters(args): Parameters<SearchArgs>,
    ) -> CallToolResult {
        self.search_impl(args, original.0).await
    }
}

#[tool_router(router = graph_search_tool_router)]
impl BarsMcp {
    #[tool(
        name = "bars_search",
        annotations(read_only_hint = true),
        description = "Search project documentation with this instead of Grep or Glob. \
                       Graph-enabled form: mode=mix adds related sources from the graph, \
                       mode=graph returns relations instead of fragments; without mode — the \
                       same three queries as the base search (question, like, occurrences). \
                       Scope with in: path prefixes (features, features/search); empty — whole \
                       project. Zero results do not prove absence: check subject, project, scope."
    )]
    async fn bars_search_graph(
        &self,
        original: observation::Original,
        Parameters(args): Parameters<GraphSearchArgs>,
    ) -> CallToolResult {
        self.graph_search_impl(args, original.0).await
    }
}
