//! `bars_read` — текст, структура или ссылки одного документа. Длинный документ без уточнений
//! возвращается структурой с подсказкой, как прочитать часть или весь текст.

use super::*;

/// Документ длиннее этого числа строк без view/lines/under отдаётся структурой, а не текстом.
const AUTO_MAX_LINES: usize = 400;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReadArgs {
    /// Path to the .md file: from the documentation root, then from the project root, as printed
    /// in search results.
    pub path: String,
    /// Part by original line numbers: N, N-M or N-; inclusive, counting from 1.
    /// Incompatible with under.
    pub lines: Option<String>,
    /// A section by heading chain; the chain ending is enough (e.g. "Scoring in results" for
    /// "Search/Scoring in results"). Includes nested content and all repeats. No or several
    /// matches — an error listing the nearest headings with their line numbers.
    pub under: Option<String>,
    /// What to return: text (default), outline (headings with line numbers) or links
    /// (direction in|out, paginated by cursor). Without view, lines and under a document of up
    /// to 400 lines comes back whole; a longer one comes back as an outline with a hint how to
    /// read a part or the whole text.
    pub view: Option<String>,
    /// For view=links: in (incoming) or out (outgoing, default).
    pub direction: Option<String>,
    /// Opaque continuation of a links page from the previous answer.
    pub cursor: Option<String>,
    pub project: Option<String>,
}

impl BarsMcp {
    async fn read_impl(&self, args: ReadArgs, original: Value) -> CallToolResult {
        let view = match args.view.as_deref() {
            None => None,
            Some("text") => Some("text"),
            Some("outline") => Some("outline"),
            Some("links") => Some("links"),
            Some(other) => {
                return CallToolResult::error(vec![ContentBlock::text(format!(
                    "invalid_params: view is text, outline or links, not {other}"
                ))]);
            }
        };
        if view == Some("links") {
            let direction = match args.direction.as_deref() {
                None | Some("out") | Some("outgoing") => "outgoing",
                Some("in") | Some("incoming") => "incoming",
                Some(other) => {
                    return CallToolResult::error(vec![ContentBlock::text(format!(
                        "invalid_params: direction is in or out, not {other}"
                    ))]);
                }
            };
            let params = bars_protocol::DiskLinksParams {
                target: self.target(args.project),
                path: args.path,
                direction: direction.to_string(),
                limit: None,
                cursor: args.cursor,
            };
            return present(
                self.call_observed(&params, Some(&original)).await,
                render::links,
            );
        }
        if view == Some("outline") {
            let params = bars_protocol::OutlineParams {
                target: self.target(args.project),
                path: args.path.clone(),
                levels: Vec::new(),
                under: args.under,
                under_line: None,
            };
            let path = params.path.clone();
            return present(
                self.call_observed(&params, Some(&original)).await,
                |output| render::outline(&path, output),
            );
        }
        let auto = args.lines.is_none() && args.under.is_none() && view.is_none();
        let params = bars_protocol::ReadParams {
            target: self.target(args.project),
            path: args.path.clone(),
            lines: args.lines,
            under: args.under,
            max_lines: auto.then_some(AUTO_MAX_LINES),
        };
        let path = params.path.clone();
        present(
            self.call_observed(&params, Some(&original)).await,
            |output| render::read(&path, output),
        )
    }
}

#[tool_router(router = read_tool_router)]
impl BarsMcp {
    #[tool(
        name = "bars_read",
        annotations(read_only_hint = true),
        description = "Read a known .md document: text, structure or links. path — from the \
                       documentation root or the project root, as printed in search results. \
                       Without view, lines and under a document of up to 400 lines comes back \
                       whole; a longer one comes back as an outline — read a part with lines \
                       (N, N-M, N-) or under a heading, or take the whole text with view=text. \
                       under accepts a heading chain or its ending; no or several matches give \
                       an error with the nearest headings and their lines. view=outline returns \
                       headings with line numbers; view=links returns the document's links \
                       (direction in|out, continue with cursor) — check them before editing. \
                       Reads saved disk, not the window's unsaved buffer."
    )]
    async fn bars_read(
        &self,
        original: observation::Original,
        Parameters(args): Parameters<ReadArgs>,
    ) -> CallToolResult {
        self.read_impl(args, original.0).await
    }
}
