//! `bars-mcp` как библиотека: stdio-MCP с шестью инструментами (Т-2, Т-19).
//!
//! Каждый инструмент — один вызов метода протокола ([`bars_protocol`]); индекса и моделей здесь нет
//! (НФТ-3). Ответ на `initialize` и список инструментов отдаются сразу, не подключаясь к воркеру;
//! подключение и запуск воркера происходят при первом вызове инструмента
//! (`docs/architecture/ipc.md`). Контракт инструментов — `docs/features/agent-tools/mcp-tools.md`.
//! Права MCP на стороне воркера задаёт `bars_protocol::Session`: `accept_model_change` от MCP
//! сбрасывается в `false`, а прокси его и не передаёт.

mod instructions;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bars_protocol::{
    Client, ClientError, ClientKind, Endpoint, ErrorKind, Heading, LAUNCH_DEADLINE, Method,
    OutlineParams, ProjectsParams, ReadParams, ReindexParams, SearchParams, StatusParams, Target,
    connect_or_launch_with, number_lines, spawn_worker, worker_exe,
};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ServerCapabilities, ServerConfig};
use rmcp::{ServerHandler, ServiceExt, schemars, tool, tool_handler, tool_router};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

pub use instructions::INSTRUCTIONS;

/// Сколько прокси ждёт ответа воркера на один вызов. Первый поиск может ждать загрузки модели.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(120);

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
}

impl Env {
    /// Окружение процесса: канал текущего пользователя, текущая папка, запуск `bars`.
    pub fn current() -> anyhow::Result<Self> {
        Ok(Self {
            endpoint: Endpoint::current()?,
            cwd: std::env::current_dir()?,
            launch: Launch::Worker,
            call_timeout: CALL_TIMEOUT,
        })
    }
}

/// Запускает MCP-сервер на stdin/stdout и работает до закрытия соединения клиентом.
pub async fn run_stdio(env: Env) -> anyhow::Result<()> {
    let service = BarsMcp::new(env).serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}

/// MCP-сервер bars. Соединение с воркером держится всю сессию и открывается при первом вызове.
#[derive(Clone)]
pub struct BarsMcp {
    env: Env,
    client: Arc<Mutex<Option<Client>>>,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct SearchArgs {
    /// Поисковый запрос своими словами.
    pub query: String,
    /// Проект: путь к корню или имя. Без него — проект сессии.
    pub project: Option<String>,
    /// Искать только в этих модулях (подпапках документации); `.` — корневой.
    #[serde(default)]
    pub modules: Vec<String>,
    /// Искать только в этих файлах, пути от корня документации.
    #[serde(default)]
    pub files: Vec<String>,
    /// Сколько результатов вернуть.
    pub limit: Option<u32>,
    /// Сколько кандидатов брать до реранка.
    pub candidates: Option<u32>,
    /// Порог оценки результата, 0…1.
    pub min_score: Option<f32>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ProjectArgs {
    /// Проект: путь к корню или имя. Без него — проект сессии.
    pub project: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ReindexArgs {
    /// Проект: путь к корню или имя. Без него — проект сессии.
    pub project: Option<String>,
    /// Полная переиндексация вместо проверки актуальности.
    #[serde(default)]
    pub full: bool,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ReadArgs {
    /// Путь к `.md`: от корня документации, затем от корня проекта.
    pub path: String,
    /// Проект: путь к корню или имя. Без него — проект сессии.
    pub project: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct OutlineArgs {
    /// Путь к `.md`: от корня документации, затем от корня проекта.
    pub path: String,
    /// Проект: путь к корню или имя. Без него — проект сессии.
    pub project: Option<String>,
    /// Какие уровни заголовков вернуть (1…6); по умолчанию все.
    #[serde(default)]
    pub levels: Vec<u8>,
    /// Только заголовки внутри этого: цепочка `Заголовок 1/Заголовок 2`, как `heading_chain` в поиске.
    pub under: Option<String>,
    /// Только заголовки внутри заголовка на этой строке.
    pub under_line: Option<u32>,
}

impl BarsMcp {
    pub fn new(env: Env) -> Self {
        Self {
            env,
            client: Arc::new(Mutex::new(None)),
            tool_router: Self::tool_router(),
        }
    }

    fn target(&self, project: Option<String>) -> Target {
        Target {
            cwd: self.env.cwd.display().to_string(),
            project: project.map(|value| project_arg(&value, &self.env.cwd)),
        }
    }

    async fn connect(&self) -> Result<Client, ClientError> {
        let launch = self.env.launch.clone();
        connect_or_launch_with(
            &self.env.endpoint,
            ClientKind::Mcp,
            LAUNCH_DEADLINE,
            move || match launch {
                Launch::Worker => spawn_worker(&worker_exe()?),
                Launch::With(start) => start(),
            },
        )
        .await
    }

    /// Вызывает метод воркера. Соединение открывается при первом вызове; оборванное (воркер
    /// перезапустили) открывается заново, и вызов повторяется один раз. После таймаута или сбоя канала
    /// соединение сбрасывается: ответ мог прийти позже и сбить порядок вызовов.
    async fn call<M: Method>(&self, params: &M) -> Result<M::Output, ClientError> {
        let mut slot = self.client.lock().await;
        for attempt in 0..2 {
            if slot.is_none() {
                *slot = Some(self.connect().await?);
            }
            let client = slot.as_mut().expect("соединение открыто выше");
            match tokio::time::timeout(self.env.call_timeout, client.call(params)).await {
                Ok(Ok(output)) => return Ok(output),
                Ok(Err(error @ (ClientError::Io(_) | ClientError::BadReply(_)))) => {
                    *slot = None;
                    if attempt == 1 {
                        return Err(error);
                    }
                }
                Ok(Err(error)) => return Err(error),
                Err(_) => {
                    *slot = None;
                    return Err(ClientError::Io(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("воркер не ответил за {} с", self.env.call_timeout.as_secs()),
                    )));
                }
            }
        }
        unreachable!("вторая попытка возвращает результат")
    }
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

/// Ответ метода как JSON — тот же, что печатает `bars-cli --json`.
fn json<T: Serialize>(output: &T) -> String {
    serde_json::to_string(output).expect("ответы методов сериализуются")
}

/// Ошибка воркера или канала — понятный ответ инструмента, а не ошибка протокола MCP: клиент
/// показывает модели текст только первого (см. `CallToolResult::error`).
fn failure(error: &ClientError) -> CallToolResult {
    let mut message = match error {
        ClientError::Rpc(rpc) => match rpc.kind() {
            Some(kind) => format!("Ошибка bars ({}): {}", kind.name(), rpc.message),
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
            Some(ErrorKind::ModelLoading) => {
                message.push_str("\nМодель ещё загружается: повторите вызов позже.")
            }
            _ => {}
        },
        ClientError::Unavailable(_) => message
            .push_str("\nПовторите вызов позже; если не помогает, запустите приложение bars."),
        _ => {}
    }
    CallToolResult::error(vec![ContentBlock::text(message)])
}

fn finish<T>(result: Result<T, ClientError>, render: impl FnOnce(&T) -> String) -> CallToolResult {
    match result {
        Ok(output) => text(render(&output)),
        Err(error) => failure(&error),
    }
}

#[tool_router]
impl BarsMcp {
    #[tool(
        name = "bars_search",
        description = "Поиск по документации проекта: гибридный (по смыслу и по словам). Возвращает \
                       фрагменты с путём, цепочкой заголовков, строками и оценкой. Параметры \
                       modules и files сужают область; без project ищет в проекте сессии."
    )]
    async fn bars_search(&self, Parameters(args): Parameters<SearchArgs>) -> CallToolResult {
        let params = SearchParams {
            target: self.target(args.project),
            query: args.query,
            modules: args.modules,
            files: args.files,
            limit: args.limit,
            candidates: args.candidates,
            min_score: args.min_score,
        };
        finish(self.call(&params).await, json)
    }

    #[tool(
        name = "bars_projects",
        description = "Зарегистрированные проекты: путь, корень документации, состояние и модули \
                       с числом документов."
    )]
    async fn bars_projects(&self) -> CallToolResult {
        finish(self.call(&ProjectsParams {}).await, json)
    }

    #[tool(
        name = "bars_status",
        description = "Состояние индекса проекта: когда обновлён, идёт ли индексация, сколько \
                       документов ждёт, модель индекса, нужна ли переиндексация, последняя ошибка."
    )]
    async fn bars_status(&self, Parameters(args): Parameters<ProjectArgs>) -> CallToolResult {
        let params = StatusParams {
            target: self.target(args.project),
        };
        finish(self.call(&params).await, json)
    }

    #[tool(
        name = "bars_reindex",
        description = "Переиндексация проекта: проверка актуальности или, с full, полная. \
                       Возвращается сразу, ход виден в bars_status. После смены модели переиндексацию \
                       запускает только пользователь: инструмент ответит ошибкой reindex_needs_consent."
    )]
    async fn bars_reindex(&self, Parameters(args): Parameters<ReindexArgs>) -> CallToolResult {
        let params = ReindexParams {
            target: self.target(args.project),
            full: args.full,
            // Согласие даёт человек (Т-25): прокси его не передаёт, а воркер от MCP всё равно сбросил бы.
            accept_model_change: false,
        };
        finish(self.call(&params).await, json)
    }

    #[tool(
        name = "bars_read",
        description = "Документ `.md` целиком, с номерами строк и title. Путь — от корня документации \
                       или от корня проекта (как в выдаче поиска)."
    )]
    async fn bars_read(&self, Parameters(args): Parameters<ReadArgs>) -> CallToolResult {
        let params = ReadParams {
            target: self.target(args.project),
            path: args.path,
        };
        finish(self.call(&params).await, |output| {
            format!("{}\n\n{}", output.title, number_lines(&output.text))
        })
    }

    #[tool(
        name = "bars_outline",
        description = "Структура документа `.md`: заголовки с номерами строк, читается с диска. \
                       levels фильтрует уровни, under (цепочка заголовков) или under_line — только \
                       вложенные в заголовок. Нужна, чтобы прочитать нужный раздел, а не весь файл."
    )]
    async fn bars_outline(&self, Parameters(args): Parameters<OutlineArgs>) -> CallToolResult {
        let params = OutlineParams {
            target: self.target(args.project),
            path: args.path,
            levels: args.levels,
            under: args.under,
            under_line: args.under_line,
        };
        finish(self.call(&params).await, |output| {
            output
                .headings
                .iter()
                .map(Heading::to_line)
                .collect::<Vec<_>>()
                .join("\n")
        })
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for BarsMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(INSTRUCTIONS)
    }
}
