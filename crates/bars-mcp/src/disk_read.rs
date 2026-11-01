use super::*;

pub(super) fn router() -> ToolRouter<BarsMcp> {
    BarsMcp::disk_tool_router()
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextArgs {
    pub project: Option<String>,
    pub format: Option<Format>,
    /// Литерал или регулярное выражение; поиск по сохранённым .md без модели.
    pub query: String,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub regex: bool,
    #[serde(default)]
    pub path_filter: String,
    /// Папки первого уровня docs-root; пусто — весь проект.
    #[serde(default)]
    pub modules: Vec<String>,
    /// Пути от docs-root (без docs/ при корне docs).
    #[serde(default)]
    pub files: Vec<String>,
    /// 20 по умолчанию; максимум 100. Ноль недопустим.
    pub limit_docs: Option<usize>,
    /// 100 по умолчанию; максимум 500. Ноль недопустим.
    pub limit_hits: Option<usize>,
    /// Продолжение одного документа: точный path ответа, от корня проекта.
    pub path: Option<String>,
    #[serde(default)]
    pub offset: usize,
    /// Ревизия предыдущей порции; изменившаяся — stale_revision.
    pub revision: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LinksArgs {
    pub project: Option<String>,
    pub format: Option<Format>,
    /// .md от корня проекта, через /; не от docs-root.
    pub path: String,
    /// outgoing (по умолчанию) или incoming; одно вхождение на запись.
    pub direction: Option<String>,
    /// 50 по умолчанию; максимум 200. Ноль недопустим.
    pub limit: Option<usize>,
    /// Непрозрачный next_cursor; при stale_cursor начать сначала.
    pub cursor: Option<String>,
}

#[tool_router(router = disk_tool_router)]
impl BarsMcp {
    #[tool(
        name = "bars_text_search",
        annotations(read_only_hint = true),
        description = "Точные вхождения в сохранённых .md с диска: литерал/regex, регистр, область. Не требует embed; не видит несохранённый редактор. До пяти вхождений на документ; продолжение одного документа через path/offset/revision. При усечении totals — нижняя граница; общей пагинации документов нет, сузьте область. Смысловой поиск — bars_rag_search."
    )]
    async fn bars_text_search(
        &self,
        original: observation::Original,
        Parameters(a): Parameters<TextArgs>,
    ) -> CallToolResult {
        let p = bars_protocol::DiskTextParams {
            target: self.target(a.project),
            query: a.query,
            case_sensitive: a.case_sensitive,
            regex: a.regex,
            path_filter: a.path_filter,
            modules: a.modules,
            files: a.files,
            limit_docs: a.limit_docs,
            limit_hits: a.limit_hits,
            path: a.path,
            offset: a.offset,
            revision: a.revision,
        };
        finish(
            self.call_observed(&p, Some(&original.0)).await,
            a.format,
            |out| render_disk("TEXT SEARCH", out),
        )
    }

    #[tool(
        name = "bars_links",
        annotations(read_only_hint = true),
        description = "Свежая страница исходящих или входящих ссылок одного .md после сверки диска. Общий индекс панели ссылок, без embed; не GraphRAG. path — от корня проекта. Проверяйте known, has_more, next_cursor и limit_reasons; stale_cursor требует начать сначала. Исходные файлы не меняются; внешние URL не открываются."
    )]
    async fn bars_links(
        &self,
        original: observation::Original,
        Parameters(a): Parameters<LinksArgs>,
    ) -> CallToolResult {
        let p = bars_protocol::DiskLinksParams {
            target: self.target(a.project),
            path: a.path,
            direction: a.direction.unwrap_or_else(|| "outgoing".into()),
            limit: a.limit,
            cursor: a.cursor,
        };
        finish(
            self.call_observed(&p, Some(&original.0)).await,
            a.format,
            |out| render_disk("LINKS", out),
        )
    }
}

// Все поля и ограничения одинаковы с JSON; текстовые границы не меняют исходное содержимое.
fn render_disk<T: Serialize>(name: &str, output: &T) -> String {
    format!(
        "=== {name} ===\n{}\n=== END {name} ===",
        serde_json::to_string_pretty(output).expect("disk output")
    )
}
