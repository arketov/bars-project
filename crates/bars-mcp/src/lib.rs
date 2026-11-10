//! `bars-mcp`: четыре агентских инструмента и шесть графовых по настройке (Т-2, Т-19, шаг 5.2.1).
//!
//! Каждый инструмент — один вызов метода протокола ([`bars_protocol`]); индекса и моделей здесь нет
//! (НФТ-3). Описания инструментов и стартовая инструкция `initialize` — на английском (НФТ-4).
//! `initialize` читает метаданные проекта с общим сроком 3 с; без воркера отвечает статичной
//! инструкцией. Пул до четырёх каналов для обычных вызовов живёт всю сессию
//! (`docs/architecture/ipc.md`). Контракт инструментов — `docs/features/agent-tools/mcp-tools.md`.
//! Права MCP на стороне воркера задаёт `bars_protocol::Session`: `accept_model_change` от MCP
//! сбрасывается в `false`, а прокси его и не передаёт.

mod graph;
mod instructions;
mod observation;
mod pool;
mod render;
mod settings;
mod tools;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bars_protocol::{
    Client, ClientError, ClientKind, Endpoint, ErrorKind, LAUNCH_DEADLINE, Method,
    connect_or_launch_with, spawn_worker, worker_exe,
};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ServerCapabilities, ServerConfig};
use rmcp::{ServerHandler, ServiceExt, schemars, tool, tool_handler, tool_router};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::Mutex;

pub use instructions::{INSTRUCTIONS, instructions};

/// Общий срок вызова, короче стандартного клиентского предела 60 с (OQ-182).
pub const CALL_TIMEOUT: Duration = Duration::from_secs(55);

/// Сколько прокси сам повторяет вызов, пока воркер отвечает `model_loading`: агенту нечем ждать,
/// а загрузка уже скачанной модели в память обычно занимает секунды (ISS-18).
pub const MODEL_WAIT: Duration = Duration::from_secs(60);

/// Пауза между повторами при `model_loading`.
const MODEL_POLL: Duration = Duration::from_secs(1);

/// Способ запустить воркер, когда к каналу не подключиться.
#[derive(Clone)]
pub enum Launch {
    /// `bars --background` рядом с exe прокси.
    Worker,
    /// Свой способ: для тестов.
    With(Arc<dyn Fn() -> io::Result<()> + Send + Sync>),
}

/// Окружение прокси: куда подключаться, откуда идёт сессия и как запускать воркер.
#[derive(Clone)]
pub struct Env {
    pub endpoint: Endpoint,
    /// Рабочая папка сессии агента; по ней воркер находит проект (Т-15).
    pub cwd: PathBuf,
    pub launch: Launch,
    pub call_timeout: Duration,
    /// Сколько повторять вызов, пока модель загружается ([`MODEL_WAIT`]).
    pub model_wait: Duration,
    /// Папка `~/.bars`: из её `settings.toml` прокси узнаёт, показывать ли графовые инструменты.
    /// `None` — не читать, инструменты скрыты.
    pub bars_home: Option<PathBuf>,
}

impl Env {
    /// Окружение процесса: канал текущего пользователя, текущая папка, запуск `bars`.
    pub fn current() -> anyhow::Result<Self> {
        Ok(Self {
            endpoint: Endpoint::current()?,
            cwd: std::env::current_dir()?,
            launch: Launch::Worker,
            call_timeout: configured_timeout()?,
            model_wait: MODEL_WAIT,
            bars_home: bars_protocol::user_home().map(|home| home.join(".bars")),
        })
    }
}

fn configured_timeout() -> anyhow::Result<Duration> {
    let mut timeout = CALL_TIMEOUT;
    for name in ["BARS_MCP_CALL_TIMEOUT_MS", "MCP_TOOL_TIMEOUT"] {
        if let Some(value) = std::env::var_os(name) {
            let millis = value
                .to_str()
                .and_then(|s| s.parse::<u64>().ok())
                .filter(|n| *n > 0)
                .ok_or_else(|| anyhow::anyhow!("{name}: нужно положительное число миллисекунд"))?;
            timeout = timeout.min(Duration::from_millis(millis));
        }
    }
    Ok(timeout)
}

/// Запускает MCP-сервер на stdin/stdout и работает до закрытия соединения клиентом.
pub async fn run_stdio(env: Env) -> anyhow::Result<()> {
    let service = BarsMcp::new(env).serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}

/// Версия и предупреждение о сборке воркера, увиденные прокси при последнем подключении.
#[derive(Clone)]
pub struct WorkerBuild {
    pub version: String,
    pub build_warning: Option<String>,
}

/// MCP-сервер bars. Каналы пула открываются по требованию и переиспользуются после ответа.
#[derive(Clone)]
pub struct BarsMcp {
    env: Env,
    pool: Arc<pool::Pool>,
    tool_router: ToolRouter<Self>,
    /// Показаны ли шесть графовых инструментов; решается при создании сессии (OQ-162).
    graph_tools: bool,
    session_info: Arc<Mutex<SessionMetadata>>,
    /// Что прокси узнал о сборке воркера из `hello` последнего нового соединения (ISS-117).
    worker_build: Arc<Mutex<Option<WorkerBuild>>>,
}

#[derive(Default)]
struct SessionMetadata {
    attempts: u8,
    project: Option<bars_protocol::SessionProjectInfo>,
}

#[tool_router(router=graph_tool_router)]
impl BarsMcp {
    #[tool(
        name = "bars_relations",
        annotations(read_only_hint = true),
        description = "Явные связи с основанием. Вход — query (имя) или \
                       entity_ref; отрицания и условия сохранены, неоднозначное имя даёт кандидатов. \
                       Пустота не доказывает отсутствия связей."
    )]
    async fn bars_relations(
        &self,
        original: observation::Original,
        Parameters(args): Parameters<graph::RelationsArgs>,
    ) -> CallToolResult {
        self.graph_call(original, args.into_call()).await
    }

    #[tool(
        name = "bars_paths",
        annotations(read_only_hint = true),
        description = "Как связаны два объекта from и to: пути с основанием каждого шага. По умолчанию \
                       смысловые рёбра; концы принимают {document:путь}. structural и mixed — в path_kind. Путь — связь, а не \
                       объяснение причины; отсутствие пути не доказывает отсутствия связи."
    )]
    async fn bars_paths(
        &self,
        original: observation::Original,
        Parameters(args): Parameters<graph::PathsArgs>,
    ) -> CallToolResult {
        self.graph_call(original, args.into_call()).await
    }

    #[tool(
        name = "bars_impact",
        annotations(read_only_hint = true),
        description = "Документы для перепроверки после правки: зависимости и проверки с основанием. \
                       Вход — documents (пути) или roots из графа. Пустота не доказывает отсутствия \
                       зависимостей. Гиперссылки — bars_read view=links direction=incoming."
    )]
    async fn bars_impact(
        &self,
        original: observation::Original,
        Parameters(args): Parameters<graph::ImpactArgs>,
    ) -> CallToolResult {
        self.graph_call(original, args.into_call()).await
    }

    #[tool(
        name = "bars_context",
        annotations(read_only_hint = true),
        description = "Первичные источники к задаче query по ролям: требование, решение, поведение, \
                       устройство, ограничение, проверка (roles, task_profile). Список для чтения \
                       в token_budget; смотри missing_roles. documents/roots сужают окрестность, \
                       query ранжирует внутри неё."
    )]
    async fn bars_context(
        &self,
        original: observation::Original,
        Parameters(args): Parameters<graph::ContextArgs>,
    ) -> CallToolResult {
        self.graph_call(original, args.into_call()).await
    }

    #[tool(
        name = "bars_explore",
        annotations(read_only_hint = true),
        description = "Обзор темы (topic) или всего проекта (whole_scope=true) для аудита: analysis=topics \
                       — темы, connectivity — изоляты и битые связи, coverage — пробелы, \
                       conflict_candidates — кандидаты в противоречия. Кандидат — повод прочитать \
                       оба места, не вывод."
    )]
    async fn bars_explore(
        &self,
        original: observation::Original,
        Parameters(args): Parameters<graph::ExploreArgs>,
    ) -> CallToolResult {
        self.graph_call(original, args.into_call()).await
    }

    #[tool(
        name = "bars_graph",
        annotations(read_only_hint = true),
        description = "Подграф документов, разделов, чанков или сущностей (view), например чтобы \
                       проверить находку bars_explore. local=true с documents или roots — окрестность на глубину \
                       max_depth. Ответ объёмный: задавай limit. Только читает готовый граф."
    )]
    async fn bars_graph(
        &self,
        original: observation::Original,
        Parameters(args): Parameters<graph::GraphViewArgs>,
    ) -> CallToolResult {
        self.graph_call(original, args.into_call()).await
    }

    /// Сессия MCP. Графовые инструменты попадают в список, только если `settings.toml` включает их
    /// (`[mcp] graph_tools`); скрытый инструмент вызвать нельзя — клиент получит «неизвестный».
    pub fn new(env: Env) -> Self {
        let graph_tools = settings::graph_tools_enabled(env.bars_home.as_deref());
        let mut tool_router = Self::tool_router() + tools::router();
        if graph_tools {
            tool_router += Self::graph_tool_router();
            tool_router += tools::search::graph_router();
        }
        Self {
            env,
            pool: Arc::new(pool::Pool::default()),
            tool_router,
            graph_tools,
            session_info: Arc::new(Mutex::new(SessionMetadata::default())),
            worker_build: Arc::new(Mutex::new(None)),
        }
    }

    fn target(&self, project: Option<String>) -> bars_protocol::Target {
        bars_protocol::Target {
            cwd: self.env.cwd.display().to_string(),
            project: project.map(|value| project_arg(&value, &self.env.cwd)),
        }
    }

    async fn connect(&self) -> Result<Client, ClientError> {
        self.connect_with_deadline(LAUNCH_DEADLINE).await
    }

    async fn connect_with_deadline(&self, deadline: Duration) -> Result<Client, ClientError> {
        let launch = self.env.launch.clone();
        connect_or_launch_with(
            &self.env.endpoint,
            ClientKind::Mcp,
            deadline,
            move || match launch {
                Launch::Worker => spawn_worker(&worker_exe()?),
                Launch::With(start) => start(),
            },
        )
        .await
    }

    async fn session_metadata(
        &self,
        max_attempts: u8,
    ) -> Option<bars_protocol::SessionProjectInfo> {
        let mut state = self.session_info.lock().await;
        if state.project.is_none() && state.attempts < max_attempts {
            state.attempts += 1;
            // Отдельный клиент: отменённый запрос не оставит поздний ответ в канале инструментов.
            let result = tokio::time::timeout(Duration::from_secs(3), async {
                let mut client = self.connect_with_deadline(Duration::from_secs(3)).await?;
                client
                    .call(&bars_protocol::SessionInfoParams {
                        cwd: self.env.cwd.display().to_string(),
                    })
                    .await
            })
            .await;
            if let Ok(Ok(output)) = result {
                state.project = output.project;
            }
        }
        state.project.clone()
    }

    /// Сборка воркера из `hello` последнего нового соединения; до первого соединения — `None`.
    async fn worker_build(&self) -> Option<WorkerBuild> {
        self.worker_build.lock().await.clone()
    }

    /// Вызывает метод воркера через пул. После обрыва чтение повторяется один раз; запись
    /// не повторяется, поскольку могла выполниться до обрыва. Незавершённый канал закрывается:
    /// поздний ответ не должен сбить порядок вызовов. Общий срок и отмена — в `call_tool`.
    /// Пока модель загружается, вызов повторяется до `model_wait`: агент получает ответ, а не
    /// совет повторить позже. `context` — исходный вход для истории, с номером попытки прокси.
    async fn call_observed<M: Method>(
        &self,
        params: &M,
        context: Option<&Value>,
    ) -> Result<M::Output, ClientError> {
        wait_for_model(self.env.model_wait, MODEL_POLL, |attempt| async move {
            let context = context.map(|context| with_attempt(context, attempt));
            self.call_once(params, context.as_ref()).await
        })
        .await
    }

    async fn call_once<M: Method>(
        &self,
        params: &M,
        context: Option<&Value>,
    ) -> Result<M::Output, ClientError> {
        let mut lease = self.pool.acquire().await;
        for attempt in 0..2 {
            if lease.client.is_none() {
                let _connect = self.pool.connect.lock().await;
                let client = self.connect().await?;
                if let Some(warning) = client.build_warning() {
                    // stdout занят протоколом MCP; stderr клиент складывает в журнал сервера.
                    eprintln!("bars-mcp: предупреждение: {warning}");
                }
                let build = WorkerBuild {
                    version: client.worker_version().to_string(),
                    build_warning: client.build_warning().map(str::to_string),
                };
                *self.worker_build.lock().await = Some(build);
                lease.client = Some(client);
            }
            let client = lease.client.as_mut().expect("соединение открыто выше");
            match client.call_observed(params, context).await {
                Ok(output) => {
                    lease.recycle();
                    return Ok(output);
                }
                Err(error @ (ClientError::Io(_) | ClientError::BadReply(_))) => {
                    lease.client = None;
                    // Запись могла выполниться до обрыва: автоматически её не повторяем.
                    if attempt == 1
                        || matches!(
                            M::NAME,
                            "reindex" | "audit_run" | "audit_cancel" | "new_topics_check" | "like"
                        )
                    {
                        return Err(error);
                    }
                }
                Err(error) => {
                    lease.recycle();
                    return Err(error);
                }
            }
        }
        unreachable!("вторая попытка возвращает результат")
    }
}

/// Повторяет `call`, пока воркер отвечает `model_loading` и не вышло время `wait`; последний ответ
/// возвращается как есть. `call` получает номер попытки с единицы.
async fn wait_for_model<T, F, Fut>(
    wait: Duration,
    poll: Duration,
    mut call: F,
) -> Result<T, ClientError>
where
    F: FnMut(u32) -> Fut,
    Fut: std::future::Future<Output = Result<T, ClientError>>,
{
    let deadline = tokio::time::Instant::now() + wait;
    let mut attempt = 0;
    loop {
        attempt += 1;
        match call(attempt).await {
            Err(ClientError::Rpc(rpc))
                if rpc.kind() == Some(ErrorKind::ModelLoading)
                    && tokio::time::Instant::now() + poll <= deadline =>
            {
                tokio::time::sleep(poll).await;
            }
            other => return other,
        }
    }
}

/// Исходный вход с номером попытки прокси: история отличает повтор прокси от повтора агента (ISS-31).
fn with_attempt(context: &Value, attempt: u32) -> Value {
    let mut context = context.clone();
    if let Some(fields) = context.as_object_mut() {
        fields.insert("proxy_attempt".into(), Value::from(attempt));
    }
    context
}

/// Значение `project`: путь (с разделителем или точкой в начале) приводится к абсолютному от рабочей
/// папки сессии, остальное — имя проекта, его разберёт воркер.
fn project_arg(value: &str, cwd: &Path) -> String {
    if value.starts_with('.') || value.contains(['/', '\\']) {
        let path = Path::new(value);
        if !path.is_absolute() {
            return cwd.join(path).display().to_string();
        }
    }
    value.to_string()
}

fn text(content: String) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(content)])
}

/// Ответ инструмента: текстовый вид результата или понятная ошибка, а не ошибка протокола MCP.
fn present<T: Serialize>(
    result: Result<T, ClientError>,
    render: impl FnOnce(&T) -> String,
) -> CallToolResult {
    match result {
        Ok(output) => text(render(&output)),
        Err(error) => failure(&error),
    }
}

/// Ошибка воркера или канала — понятный ответ инструмента, а не ошибка протокола MCP: клиент
/// показывает модели текст только первого (см. `CallToolResult::error`).
fn failure(error: &ClientError) -> CallToolResult {
    let mut message = match error {
        ClientError::Rpc(rpc) => match rpc.kind() {
            // Уточнённый вид из `data.kind` (`too_broad`, `invalid_regex`) говорит агенту больше кода.
            Some(kind) => format!(
                "Ошибка bars ({}): {}",
                rpc.data
                    .as_ref()
                    .and_then(|d| d.get("kind"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(kind.name()),
                rpc.message
            ),
            None => format!("Ошибка bars: {}", rpc.message),
        },
        other => format!("Ошибка bars: {other}"),
    };
    match error {
        ClientError::Rpc(rpc) => match rpc.kind() {
            Some(ErrorKind::ReindexRequired | ErrorKind::ReindexNeedsConsent) => message.push_str(
                "\nИндекс построен другой моделью. Агент переиндексацию запустить не может: \
                 нужно согласие пользователя. Сообщите ему и не повторяйте вызов; он подтвердит \
                 в окне bars или командой `bars-cli reindex --accept-model-change`.",
            ),
            Some(ErrorKind::ModelLoading) => message.push_str(
                "\nМодель не загрузилась за время ожидания вызова: скорее всего, она ещё \
                 скачивается. Повторите вызов позже.",
            ),
            _ => {}
        },
        ClientError::Unavailable(_) => message
            .push_str("\nПовторите вызов позже; если не помогает, запустите приложение bars."),
        _ => {}
    }
    CallToolResult::error(vec![ContentBlock::text(message)])
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for BarsMcp {
    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::CallToolResponse, rmcp::ErrorData> {
        let cancelled = context.ct.clone();
        let timeout = self.env.call_timeout.min(CALL_TIMEOUT);
        let call = self
            .tool_router
            .call(rmcp::handler::server::tool::ToolCallContext::new(
                self, request, context,
            ));
        tokio::select! {
            biased;
            _ = cancelled.cancelled() => Err(rmcp::ErrorData::internal_error("Вызов bars отменён клиентом", None)),
            result = tokio::time::timeout(timeout, call) => match result {
                Ok(result) => result,
                Err(_) => Ok(failure(&ClientError::Io(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("истёк общий срок вызова bars: {} мс", timeout.as_millis()),
                ))).into()),
            },
        }
    }

    async fn initialize(
        &self,
        request: rmcp::model::InitializeRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::InitializeResult, rmcp::ErrorData> {
        context.peer.set_peer_info(request.clone());
        let mut result = self.negotiate_initialize(&request)?;
        let project = self.session_metadata(1).await;
        result.instructions = Some(instructions::session_instructions(
            self.graph_tools,
            project.as_ref(),
        ));
        Ok(result)
    }

    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::ListToolsResult, rmcp::ErrorData> {
        // Список модулей проекта больше не подставляется в описания параметров: области поиска —
        // произвольные пути (`in`), а модули с назначением живут в initialize и bars_status.
        let _ = self.session_metadata(2).await;
        Ok(rmcp::model::ListToolsResult {
            tools: self.tool_router.list_all(),
            ..Default::default()
        })
    }

    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(instructions(self.graph_tools))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bars_protocol::RpcError;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn loading() -> ClientError {
        ClientError::Rpc(RpcError::bars(
            ErrorKind::ModelLoading,
            "модель загружается в память",
        ))
    }

    #[tokio::test]
    async fn model_loading_is_retried_until_the_model_answers() {
        let calls = AtomicUsize::new(0);
        let result = wait_for_model(
            Duration::from_secs(5),
            Duration::from_millis(10),
            |_| async {
                match calls.fetch_add(1, Ordering::SeqCst) {
                    0 | 1 => Err(loading()),
                    _ => Ok(42),
                }
            },
        )
        .await;
        assert_eq!(result.unwrap(), 42);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn every_retry_carries_its_proxy_attempt_number() {
        let seen = std::sync::Mutex::new(Vec::new());
        let original = serde_json::json!({"query": "запрос"});
        let result = wait_for_model(
            Duration::from_secs(5),
            Duration::from_millis(10),
            |attempt| {
                let context = with_attempt(&original, attempt);
                seen.lock().unwrap().push(context);
                async move { if attempt < 3 { Err(loading()) } else { Ok(()) } }
            },
        )
        .await;
        assert!(result.is_ok());
        let seen = seen.into_inner().unwrap();
        let attempts: Vec<_> = seen.iter().map(|c| c["proxy_attempt"].clone()).collect();
        assert_eq!(attempts, [1, 2, 3]);
        assert!(seen.iter().all(|c| c["query"] == "запрос"));
        assert_eq!(
            original.get("proxy_attempt"),
            None,
            "исходный вход не меняется"
        );
    }

    #[tokio::test]
    async fn model_loading_past_the_wait_is_returned_to_the_agent() {
        let calls = AtomicUsize::new(0);
        let result: Result<(), _> = wait_for_model(
            Duration::from_millis(50),
            Duration::from_millis(20),
            |_| async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err(loading())
            },
        )
        .await;
        assert!(
            matches!(result, Err(ClientError::Rpc(rpc)) if rpc.kind() == Some(ErrorKind::ModelLoading))
        );
        assert!((2..=3).contains(&calls.load(Ordering::SeqCst)));
    }

    #[tokio::test]
    async fn other_errors_are_not_retried() {
        let calls = AtomicUsize::new(0);
        let result: Result<(), _> = wait_for_model(
            Duration::from_secs(5),
            Duration::from_millis(10),
            |_| async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err(ClientError::Rpc(RpcError::bars(
                    ErrorKind::ProviderError,
                    "нет файлов",
                )))
            },
        )
        .await;
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
