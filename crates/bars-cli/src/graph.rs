use super::*;
use bars_protocol::graph_types::{Endpoint as GraphEndpoint, GraphReply, GraphRequest, Operation};
use bars_protocol::{GraphBuildParams, GraphCancelParams, GraphQueryParams, GraphStatusParams};

#[derive(Debug, Args)]
pub struct GraphArgs {
    /// Вопрос для relations/context; тема для explore.
    pub query: Vec<String>,
    /// Полный GraphRequest JSON. Явные CLI-флаги имеют приоритет.
    #[arg(long, value_name = "JSON-ФАЙЛ")]
    pub input: Option<PathBuf>,
    #[arg(long)]
    pub from: Option<String>,
    #[arg(long)]
    pub to: Option<String>,
    /// ObjectRef JSON из предыдущего ответа; можно повторять.
    #[arg(long = "root")]
    pub roots: Vec<String>,
    #[arg(long)]
    pub entity_ref: Option<String>,
    #[arg(long = "module")]
    pub modules: Vec<String>,
    #[arg(long = "file")]
    pub files: Vec<String>,
    #[arg(long)]
    pub view: Option<String>,
    #[arg(long)]
    pub analysis: Option<String>,
    #[arg(long)]
    pub local: bool,
    #[arg(long)]
    pub whole_scope: bool,
    #[arg(long)]
    pub max_depth: Option<u32>,
    #[arg(long)]
    pub max_bytes: Option<u32>,
    #[arg(long)]
    pub limit: Option<u32>,
    #[arg(long)]
    pub snapshot: Option<String>,
    #[arg(long)]
    pub cursor: Option<String>,
    #[arg(long)]
    pub token_budget: Option<u32>,
    #[arg(long, value_parser = ["implementation", "investigation", "decision"])]
    pub task_profile: Option<String>,
    #[arg(long)]
    pub path_kind: Option<String>,
}
#[derive(Debug, Subcommand)]
pub enum GraphAction {
    /// Общий или локальный подграф.
    Show(GraphArgs),
    /// Явно перестроить граф без переиндексации embeddings.
    Build {
        #[arg(long, default_value = "all", value_parser = ["all", "structure", "semantic", "similarity"])]
        layer: String,
    },
    /// Остановить принятое задание по ID; запрос не означает подтверждённую остановку.
    Cancel { job_id: String },
    /// Состояние слоя и фонового задания.
    Status,
}
fn invalid(message: impl ToString) -> Failure {
    Failure::Client(ClientError::Rpc(bars_protocol::RpcError::bars(
        bars_protocol::ErrorKind::InvalidParams,
        message.to_string(),
    )))
}
fn enum_value<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, Failure> {
    serde_json::from_value(serde_json::Value::String(text.into())).map_err(invalid)
}
fn endpoint(text: String) -> Result<GraphEndpoint, Failure> {
    if text.trim_start().starts_with('{') {
        serde_json::from_str(&text).map_err(invalid)
    } else {
        Ok(GraphEndpoint::Name {
            label: text,
            kind: None,
            namespace: None,
        })
    }
}
pub(super) async fn query(
    client: &mut Client,
    target: Target,
    json: bool,
    out: &mut dyn Write,
    args: GraphArgs,
    operation: Operation,
) -> Result<(), Failure> {
    let mut request: GraphRequest = if let Some(file) = args.input {
        serde_json::from_str(&std::fs::read_to_string(file).map_err(invalid)?).map_err(invalid)?
    } else {
        Default::default()
    };
    request.operation = operation;
    if !args.query.is_empty() {
        if operation == Operation::Explore {
            request.topic = Some(args.query.join(" "));
        } else {
            request.query = Some(args.query.join(" "));
        }
    }
    if let Some(value) = args.from {
        request.from = Some(endpoint(value)?);
    }
    if let Some(value) = args.to {
        request.to = Some(endpoint(value)?);
    }
    if !args.roots.is_empty() {
        request.roots = args
            .roots
            .iter()
            .map(|v| serde_json::from_str(v).map_err(invalid))
            .collect::<Result<_, _>>()?;
    }
    if let Some(value) = args.entity_ref {
        request.entity_ref = Some(serde_json::from_str(&value).map_err(invalid)?);
    }
    if !args.modules.is_empty() {
        request.scope.modules = args.modules;
    }
    if !args.files.is_empty() {
        request.scope.files = args.files;
    }
    if let Some(value) = args.view {
        request.view = enum_value(&value)?;
    }
    if let Some(value) = args.analysis {
        request.analysis = enum_value(&value)?;
    }
    if let Some(value) = args.path_kind {
        request.path_kind = enum_value(&value)?;
    }
    request.local |= args.local;
    request.whole_scope |= args.whole_scope;
    if args.max_depth.is_some() {
        request.budgets.max_depth = args.max_depth;
    }
    if args.max_bytes.is_some() {
        request.budgets.max_bytes = args.max_bytes;
    }
    if args.limit.is_some() {
        request.limit = args.limit;
    }
    if args.snapshot.is_some() {
        request.snapshot = args.snapshot;
    }
    if args.cursor.is_some() {
        request.cursor = args.cursor;
    }
    if args.token_budget.is_some() {
        request.token_budget = args.token_budget;
    }
    if let Some(profile) = args.task_profile {
        request.task_profile = Some(enum_value(&profile)?);
    }
    let result = call(client, &GraphQueryParams { target, request }).await?;
    emit(out, json, &result, render)
}
pub(super) async fn action(
    client: &mut Client,
    target: Target,
    json: bool,
    out: &mut dyn Write,
    action: GraphAction,
) -> Result<(), Failure> {
    match action {
        GraphAction::Show(args) => query(client, target, json, out, args, Operation::Graph).await,
        GraphAction::Build { layer } => emit(
            out,
            json,
            &call(
                client,
                &GraphBuildParams {
                    target,
                    layer: enum_value(&layer)?,
                },
            )
            .await?,
            |r| {
                format!(
                    "Задание принято: {}. Готовность проверяйте graph status.",
                    r.job_id
                )
            },
        ),
        GraphAction::Cancel { job_id } => emit(
            out,
            json,
            &call(client, &GraphCancelParams { target, job_id }).await?,
            |r| {
                if r.cancelled {
                    "Остановка запрошена; проверьте graph status.".into()
                } else {
                    "Активное задание не найдено.".into()
                }
            },
        ),
        GraphAction::Status => emit(
            out,
            json,
            &call(client, &GraphStatusParams { target }).await?,
            |r| {
                let mut text = format!(
                    "Граф: {}; источников: {}; задание: {}; ошибка: {}",
                    r.state.label(),
                    r.source_count,
                    r.active_job.as_deref().unwrap_or("нет"),
                    r.error.as_deref().unwrap_or("нет")
                );
                if r.paused {
                    text.push_str("\nАвтообновление остановлено; graph build продолжит обработку.");
                }
                if let Some(p) = &r.progress {
                    let phase = match p.phase.as_str() {
                        "structure" => "разбор документов",
                        "similarity" => "близость",
                        "publish" => "публикация",
                        _ => "ожидание",
                    };
                    text.push_str(&format!(
                        "\nХод: {phase}, {} / {}, {} мс",
                        p.processed,
                        p.total.map_or("?".into(), |n| n.to_string()),
                        p.elapsed_ms
                    ));
                }
                for (name, layer) in &r.layers {
                    text.push_str(&format!(
                        "\n{name}: {}, документов {} / {}, ошибок {}",
                        layer.state.label(),
                        layer.processed,
                        layer.total,
                        layer.failed
                    ));
                }
                text.push_str(&format!("\nФайлов с ошибками: {}", r.file_errors_total));
                for error in &r.file_errors {
                    text.push_str(&format!("\n{}: {}", error.path, error.message));
                }
                text
            },
        ),
    }
}
fn render(reply: &GraphReply) -> String {
    // JSON сохраняет все типы/доказательства и в текстовом режиме; человек видит границы и partial.
    format!(
        "Граф {} · {} · элементов {} · усечён: {}\n{}",
        reply.operation.label(),
        reply.layer_state.label(),
        reply.items.len(),
        reply.truncated,
        serde_json::to_string_pretty(reply).expect("DTO сериализуется")
    )
}
