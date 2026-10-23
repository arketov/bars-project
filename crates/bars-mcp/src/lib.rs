//! `bars-mcp` как библиотека: stdio-MCP с шестнадцатью инструментами (Т-2, Т-19).
//!
//! Каждый инструмент — один вызов метода протокола ([`bars_protocol`]); индекса и моделей здесь нет
//! (НФТ-3). Ответ на `initialize` и список инструментов отдаются сразу, не подключаясь к воркеру;
//! подключение и запуск воркера происходят при первом вызове инструмента
//! (`docs/architecture/ipc.md`). Контракт инструментов — `docs/features/agent-tools/mcp-tools.md`.
//! Права MCP на стороне воркера задаёт `bars_protocol::Session`: `accept_model_change` от MCP
//! сбрасывается в `false`, а прокси его и не передаёт.

mod disk_read;
mod graph;
mod instructions;
mod observation;
mod render;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bars_protocol::{
    Client, ClientError, ClientKind, DuplicatesParams, Endpoint, ErrorKind, LAUNCH_DEADLINE,
    Method, OutlineParams, ProjectsParams, ReadParams, ReindexParams, SearchParams, SimilarParams,
    StatusParams, Target, connect_or_launch_with, spawn_worker, worker_exe,
};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ServerCapabilities, ServerConfig};
use rmcp::{ServerHandler, ServiceExt, schemars, tool, tool_handler, tool_router};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

pub use instructions::INSTRUCTIONS;

/// Сколько прокси ждёт ответа воркера на один вызов. Первый поиск может ждать загрузки модели.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(120);

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
}

impl Env {
    /// Окружение процесса: канал текущего пользователя, текущая папка, запуск `bars`.
    pub fn current() -> anyhow::Result<Self> {
        Ok(Self {
            endpoint: Endpoint::current()?,
            cwd: std::env::current_dir()?,
            launch: Launch::Worker,
            call_timeout: CALL_TIMEOUT,
            model_wait: MODEL_WAIT,
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

/// Вид ответа инструмента.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// Текст с явными границами блоков (по умолчанию).
    #[default]
    Txt,
    /// JSON ответа воркера одной строкой.
    Json,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct NoArgs {
    /// Вид ответа: `txt` (по умолчанию) или `json`.
    pub format: Option<Format>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct SearchArgs {
    /// Явный hybrid/mix/graph. Без поля — прежний поиск и прежняя форма ответа.
    pub mode: Option<bars_graph::RetrievalMode>,
    #[serde(default)]
    pub graph_budgets: bars_graph::Budgets,
    /// В mix: отключить расширение и пакеты оснований; без поля включено.
    pub include_related: Option<bool>,
    /// В mix: предел байт пакета оснований, не увеличивает graph_budgets.max_bytes.
    pub related_budget: Option<u32>,
    /// Предметный вопрос на языке пользователя после определения задачи; не шаблон «этап N требования ошибки пожелания».
    pub query: String,
    /// Проект: путь к корню или имя. Без него — проект сессии.
    pub project: Option<String>,
    /// Существующие папки первого уровня docs-root из bars_projects; `.` — корневой. Примеры: ["backlog"] для пожеланий, ["chats"] для истории. Для межслойной задачи можно не ограничивать.
    #[serde(default)]
    pub modules: Vec<String>,
    /// Уже выбранные индексируемые документы, пути от docs-root, без docs/ при корне docs. Пример сверки: ["features/models/local-models.md", "features/settings/settings.md"]. Не угадывать; для целого документа — bars_read.
    #[serde(default)]
    pub files: Vec<String>,
    /// Сколько результатов вернуть.
    pub limit: Option<u32>,
    /// Сколько кандидатов брать до реранка.
    pub candidates: Option<u32>,
    /// Порог оценки результата, 0…1.
    pub min_score: Option<f32>,
    /// Вид ответа: `txt` (по умолчанию) или `json`.
    pub format: Option<Format>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct SimilarArgs {
    /// ID чанка (поле `id` результата bars_rag_search или bars_get_similar); действует, пока не изменился документ чанка.
    pub chunk_id: String,
    /// Проект: путь к корню или имя. Без него — проект сессии.
    pub project: Option<String>,
    /// Существующие папки первого уровня docs-root; `.` — корневой. Ограничивает область кандидатов, исходный чанк может быть в другом модуле. Без поля — весь проект.
    #[serde(default)]
    pub modules: Vec<String>,
    /// Сколько похожих чанков вернуть.
    pub limit: Option<u32>,
    /// Вид ответа: `txt` (по умолчанию) или `json`.
    pub format: Option<Format>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct DuplicatesArgs {
    /// Проект: путь к корню или имя. Обязателен: аудит читает весь проект.
    pub project: String,
    /// Существующие папки первого уровня docs-root; `.` — корневой. Без поля — весь проект.
    #[serde(default)]
    pub modules: Vec<String>,
    /// Сколько записей вернуть.
    pub limit: Option<u32>,
    /// Вид ответа: `txt` (по умолчанию) или `json`.
    pub format: Option<Format>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ProjectArgs {
    /// Проект: путь к корню или имя. Без него — проект сессии.
    pub project: Option<String>,
    /// Вид ответа: `txt` (по умолчанию) или `json`.
    pub format: Option<Format>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ReindexArgs {
    /// Проект: путь к корню или имя. Без него — проект сессии.
    pub project: Option<String>,
    /// Полная переиндексация вместо проверки актуальности.
    #[serde(default)]
    pub full: bool,
    /// Вид ответа: `txt` (по умолчанию) или `json`.
    pub format: Option<Format>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ReadArgs {
    /// Путь к `.md`: от корня документации, затем от корня проекта.
    pub path: String,
    /// Проект: путь к корню или имя. Без него — проект сессии.
    pub project: Option<String>,
    /// Вид ответа: `txt` (по умолчанию) или `json`.
    pub format: Option<Format>,
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
    /// Вид ответа: `txt` (по умолчанию) или `json`.
    pub format: Option<Format>,
}

#[tool_router(router=graph_tool_router)]
impl BarsMcp {
    #[tool(
        name = "bars_relations",
        description = "Явные отношения с исходными доказательствами. Ровно один query/entity_ref; отрицания и условия сохраняются. Неоднозначное имя возвращает кандидатов. Только чтение готового графа."
    )]
    async fn bars_relations(
        &self,
        Parameters(args): Parameters<graph::GraphArgs>,
    ) -> CallToolResult {
        self.graph_call(args, bars_graph::Operation::Relations)
            .await
    }

    #[tool(
        name = "bars_paths",
        description = "Ограниченные пути между from/to: ObjectRef либо label/kind/namespace. По умолчанию смысловые рёбра; structural/mixed выбираются явно. Evidence каждого шага, partial и scope; без построения графа."
    )]
    async fn bars_paths(&self, Parameters(args): Parameters<graph::GraphArgs>) -> CallToolResult {
        self.graph_call(args, bars_graph::Operation::Paths).await
    }

    #[tool(
        name = "bars_impact",
        description = "Документированное влияние изменения roots: обратные зависимости и полные пути с evidence. Гиперссылки не означают зависимости, результат не доказывает дефект кода. Только чтение."
    )]
    async fn bars_impact(&self, Parameters(args): Parameters<graph::GraphArgs>) -> CallToolResult {
        self.graph_call(args, bars_graph::Operation::Impact).await
    }

    #[tool(
        name = "bars_context",
        description = "Контекст query по ролям requirement/decision/behavior/architecture/constraint/verification. Первичные источники, общий byte/token budget, missing_roles. Без генерации ответа и скрытого обогащения."
    )]
    async fn bars_context(&self, Parameters(args): Parameters<graph::GraphArgs>) -> CallToolResult {
        self.graph_call(args, bars_graph::Operation::Context).await
    }

    #[tool(
        name = "bars_explore",
        description = "Обзор темы topic либо whole_scope=true: topics/connectivity/coverage/conflict_candidates. Структурные дефекты отдельно от кандидатов смысловых пробелов; неполнота extraction видна. Только чтение."
    )]
    async fn bars_explore(&self, Parameters(args): Parameters<graph::GraphArgs>) -> CallToolResult {
        self.graph_call(args, bars_graph::Operation::Explore).await
    }

    #[tool(
        name = "bars_graph",
        description = "Общий/локальный граф documents/sections/chunks/entities/mixed. Local требует roots; depth ограничен budgets.max_depth. Связи типизированы, поколения и scope фиксированы; курсор только того же запроса. Граф не строится вызовом чтения."
    )]
    async fn bars_graph(&self, Parameters(args): Parameters<graph::GraphArgs>) -> CallToolResult {
        self.graph_call(args, bars_graph::Operation::Graph).await
    }

    pub fn new(env: Env) -> Self {
        Self {
            env,
            client: Arc::new(Mutex::new(None)),
            tool_router: Self::tool_router() + Self::graph_tool_router() + disk_read::router(),
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
        self.call_observed(params, None).await
    }
    /// Пока модель загружается, вызов повторяется до `model_wait`: агент получает ответ, а не
    /// совет повторить позже.
    async fn call_observed<M: Method>(
        &self,
        params: &M,
        context: Option<&Value>,
    ) -> Result<M::Output, ClientError> {
        wait_for_model(self.env.model_wait, MODEL_POLL, || {
            self.call_once(params, context)
        })
        .await
    }

    async fn call_once<M: Method>(
        &self,
        params: &M,
        context: Option<&Value>,
    ) -> Result<M::Output, ClientError> {
        let mut slot = self.client.lock().await;
        for attempt in 0..2 {
            if slot.is_none() {
                *slot = Some(self.connect().await?);
            }
            let client = slot.as_mut().expect("соединение открыто выше");
            match tokio::time::timeout(self.env.call_timeout, client.call_observed(params, context))
                .await
            {
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

/// Повторяет `call`, пока воркер отвечает `model_loading` и не вышло время `wait`; последний ответ
/// возвращается как есть.
async fn wait_for_model<T, F, Fut>(
    wait: Duration,
    poll: Duration,
    mut call: F,
) -> Result<T, ClientError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, ClientError>>,
{
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        match call().await {
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

fn finish<T: Serialize>(
    result: Result<T, ClientError>,
    format: Option<Format>,
    render: impl FnOnce(&T) -> String,
) -> CallToolResult {
    match result {
        Ok(output) => text(match format.unwrap_or_default() {
            Format::Txt => render(&output),
            Format::Json => json(&output),
        }),
        Err(error) => failure(&error),
    }
}

#[tool_router]
impl BarsMcp {
    #[tool(
        name = "bars_rag_search",
        description = "Поиск неизвестного контекста после определения предмета задачи; для известного \
                       документа можно сразу bars_read/bars_outline. Гибридный поиск возвращает \
                       фрагменты с источниками, строками и оценкой. modules выбирает существующие \
                       папки первого уровня: например, [\"backlog\"] для пожеланий или [\"chats\"] \
                       для истории. files — уже выбранные индексируемые документы: например, \
                       [\"features/models/local-models.md\",\"features/settings/settings.md\"] для \
                       сверки устройства rerank; пути от docs-root. Не угадывать список файлов; \
                       межслойную задачу можно искать без фильтров. Без project — проект сессии."
    )]
    async fn bars_rag_search(
        &self,
        original: observation::Original,
        Parameters(args): Parameters<SearchArgs>,
    ) -> CallToolResult {
        if args.mode.is_none() && (args.include_related.is_some() || args.related_budget.is_some())
        {
            return CallToolResult::error(vec![ContentBlock::text(
                "include_related/related_budget требуют mode=mix",
            )]);
        }
        let mode = args.mode;
        let graph_budgets = args.graph_budgets;
        let include_related = args.include_related.unwrap_or(true);
        let related_budget = args.related_budget;
        let params = SearchParams {
            target: self.target(args.project),
            query: args.query,
            modules: args.modules,
            files: args.files,
            limit: args.limit,
            candidates: args.candidates,
            min_score: args.min_score,
        };
        if let Some(mode) = mode {
            finish(
                self.call_observed(
                    &bars_protocol::GraphSearchParams {
                        search: params,
                        mode,
                        graph_budgets,
                        include_related,
                        related_budget,
                    },
                    Some(&original.0),
                )
                .await,
                args.format,
                |out| format!("=== GRAPH SEARCH ===\n{}\n=== END SEARCH ===", json(out)),
            )
        } else {
            finish(
                self.call_observed(&params, Some(&original.0)).await,
                args.format,
                render::search,
            )
        }
    }

    #[tool(
        name = "bars_get_similar",
        description = "Похожие чанки по ID чанка из выдачи bars_rag_search (поле id): перед правкой —                        найти похожие описания и разделы для проверки, перед добавлением текста —                        существующее описание. Возвращает ближайшие чанки выбранной области (modules),                        исключая исходный; оценка — косинусная близость к исходному чанку. Близость не                        доказывает дублирование, согласие или зависимость документов: читай найденное.                        ID действует, пока не изменился документ чанка; при chunk_not_found найди чанк                        поиском заново. Без project — проект сессии."
    )]
    async fn bars_get_similar(
        &self,
        original: observation::Original,
        Parameters(args): Parameters<SimilarArgs>,
    ) -> CallToolResult {
        let params = SimilarParams {
            target: self.target(args.project),
            chunk_id: args.chunk_id,
            modules: args.modules,
            limit: args.limit,
        };
        finish(
            self.call_observed(&params, Some(&original.0)).await,
            args.format,
            render::similar,
        )
    }

    #[tool(
        name = "bars_get_duplicates",
        description = "Аудит дублей проекта: повторяющиеся и разошедшиеся описания, чтобы выбрать основной источник и заменить повторы ссылками. Без исходного чанка; project обязателен, modules ограничивает область. Виды: exact — одинаковый текст; near — почти одинаковые копии (с различиями слов, чисел и отрицаний); contained — один раздел внутри другого; partial — скопирована заметная часть; semantic — близки по смыслу, только повод прочитать оба. Наиболее явные первыми; одинаковый блок в четырёх и больше разделах помечен repeated_block (скорее шаблон). Ничего не удаляет и не объединяет. Короткие разделы (меньше 15 слов) не сравниваются."
    )]
    async fn bars_get_duplicates(
        &self,
        original: observation::Original,
        Parameters(args): Parameters<DuplicatesArgs>,
    ) -> CallToolResult {
        let params = DuplicatesParams {
            target: self.target(Some(args.project)),
            modules: args.modules,
            limit: args.limit,
        };
        finish(
            self.call_observed(&params, Some(&original.0)).await,
            args.format,
            render::duplicates,
        )
    }

    #[tool(
        name = "bars_projects",
        description = "Зарегистрированные проекты: путь, корень документации, состояние и существующие \
                       модули первого уровня с числом документов. Используй для выбора project и \
                       modules, когда их имена неизвестны; вложенная папка не отдельный модуль."
    )]
    async fn bars_projects(
        &self,
        original: observation::Original,
        Parameters(args): Parameters<NoArgs>,
    ) -> CallToolResult {
        finish(
            self.call_observed(&ProjectsParams {}, Some(&original.0))
                .await,
            args.format,
            render::projects,
        )
    }

    #[tool(
        name = "bars_status",
        description = "Состояние индекса проекта: когда обновлён, идёт ли индексация, сколько \
                       документов ждёт, модель индекса, нужна ли переиндексация, последняя ошибка."
    )]
    async fn bars_status(
        &self,
        original: observation::Original,
        Parameters(args): Parameters<ProjectArgs>,
    ) -> CallToolResult {
        let params = StatusParams {
            target: self.target(args.project),
        };
        finish(
            self.call_observed(&params, Some(&original.0)).await,
            args.format,
            render::status,
        )
    }

    #[tool(
        name = "bars_reindex",
        description = "Переиндексация проекта: проверка актуальности или, с full, полная. \
                       Возвращается сразу, ход виден в bars_status. После смены модели переиндексацию \
                       запускает только пользователь: инструмент ответит ошибкой reindex_needs_consent."
    )]
    async fn bars_reindex(
        &self,
        original: observation::Original,
        Parameters(args): Parameters<ReindexArgs>,
    ) -> CallToolResult {
        let params = ReindexParams {
            target: self.target(args.project),
            full: args.full,
            // Согласие даёт человек (Т-25): прокси его не передаёт, а воркер от MCP всё равно сбросил бы.
            accept_model_change: false,
        };
        finish(
            self.call_observed(&params, Some(&original.0)).await,
            args.format,
            render::reindex,
        )
    }

    #[tool(
        name = "bars_read",
        description = "Прочитать известный документ `.md` целиком с номерами строк и title, без \
                       обязательного предварительного поиска. Подходит для плана/реестра, чтобы \
                       определить предмет этапа. Читает с диска, включая файлы вне индекса; path — \
                       от корня документации или проекта (как в выдаче). Для заголовков — bars_outline."
    )]
    async fn bars_read(
        &self,
        original: observation::Original,
        Parameters(args): Parameters<ReadArgs>,
    ) -> CallToolResult {
        let params = ReadParams {
            target: self.target(args.project),
            path: args.path,
        };
        let path = params.path.clone();
        finish(
            self.call_observed(&params, Some(&original.0)).await,
            args.format,
            |output| render::read(&path, output),
        )
    }

    #[tool(
        name = "bars_outline",
        description = "Заголовки известного `.md` с номерами строк; можно вызвать сразу без поиска, \
                       например для определения предмета этапа по плану. Читается с диска, включая \
                       файлы вне индекса; path — от корня документации или проекта. \
                       levels фильтрует уровни, under (цепочка заголовков) или under_line — только \
                       вложенные в заголовок. Для полного текста — bars_read."
    )]
    async fn bars_outline(
        &self,
        original: observation::Original,
        Parameters(args): Parameters<OutlineArgs>,
    ) -> CallToolResult {
        let params = OutlineParams {
            target: self.target(args.project),
            path: args.path,
            levels: args.levels,
            under: args.under,
            under_line: args.under_line,
        };
        let path = params.path.clone();
        finish(
            self.call_observed(&params, Some(&original.0)).await,
            args.format,
            |output| render::outline(&path, output),
        )
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for BarsMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(INSTRUCTIONS)
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
            || async {
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
    async fn model_loading_past_the_wait_is_returned_to_the_agent() {
        let calls = AtomicUsize::new(0);
        let result: Result<(), _> = wait_for_model(
            Duration::from_millis(50),
            Duration::from_millis(20),
            || async {
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
            || async {
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
