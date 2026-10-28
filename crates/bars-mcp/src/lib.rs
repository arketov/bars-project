//! `bars-mcp` как библиотека: stdio-MCP с десятью инструментами и шестью графовыми, которые скрыты,
//! пока `settings.toml` не включит их (Т-2, Т-19, OQ-162).
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
mod settings;

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

pub use instructions::{INSTRUCTIONS, instructions};

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
            call_timeout: CALL_TIMEOUT,
            model_wait: MODEL_WAIT,
            bars_home: bars_protocol::user_home().map(|home| home.join(".bars")),
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
    /// Показаны ли шесть графовых инструментов; решается при создании сессии (OQ-162).
    graph_tools: bool,
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
    /// hybrid — обычный поиск, та же выдача, что без поля, в форме ответа с mode; mix — плюс связанные источники графа (объёмно, задай related_budget); graph — отношения вместо фрагментов. Без поля — обычный поиск с блоками RESULT.
    pub mode: Option<bars_graph::RetrievalMode>,
    /// Только для mix и graph: пределы обхода графа.
    #[serde(default)]
    pub graph_budgets: bars_graph::Budgets,
    /// Только для mix: false — без связанных источников; по умолчанию true.
    pub include_related: Option<bool>,
    /// Только для mix: предел байт пакета связанных источников, не больше graph_budgets.max_bytes. Слишком малый — выдача без пакета с fallback_reason.
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
        description = "Что с чем связано явным утверждением («от чего зависит», «что заменяет»): \
                       факты с основанием из текста. Вход — ровно одно из query (имя или вопрос) и \
                       entity_ref; отрицания и условия сохраняются. Неоднозначное имя возвращает \
                       кандидатов, а не догадку. Смысловой слой частичный: пустой ответ не \
                       доказывает, что отношений нет."
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
        description = "Как связаны два объекта: ограниченные пути между from и to (объект из \
                       ответа графа либо подпись label) с основанием каждого шага. По умолчанию \
                       смысловые рёбра, structural и mixed — явно в path_kind. Путь — связь, а не \
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
        description = "Какие документы перепроверить после правки объекта: обратные зависимости и \
                       проверки с основанием. Вход — roots из ответа другой графовой операции. \
                       Смысловой слой частичный: пустой ответ не значит, что зависимостей нет; \
                       гиперссылки сюда не входят — их даёт bars_links с direction=incoming."
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
        description = "Собрать первичные источники к задаче query по ролям: требование, решение, \
                       поведение, устройство, ограничение, проверка (roles, task_profile). Не \
                       готовый ответ, а список для чтения в пределах token_budget; смотри \
                       missing_roles."
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
        description = "Обзор темы (topic) или всего проекта (whole_scope=true) для аудита \
                       документации: analysis=topics — темы, connectivity — изолированные документы и \
                       битые связи, coverage — пробелы, conflict_candidates — кандидаты в \
                       противоречия. Кандидат — повод прочитать оба места, не вывод."
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
        description = "Подграф документов, разделов, чанков или сущностей (view), например чтобы \
                       проверить находку bars_explore. local=true с roots — окрестность объектов на \
                       глубину max_depth; без local — общий граф в пределах бюджетов. Ответ \
                       объёмный: задавай limit. Вызов только читает готовый граф и не строит его."
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
        let mut tool_router = Self::tool_router() + disk_read::router();
        if graph_tools {
            tool_router += Self::graph_tool_router();
        }
        Self {
            env,
            client: Arc::new(Mutex::new(None)),
            tool_router,
            graph_tools,
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
        let mut slot = self.client.lock().await;
        for attempt in 0..2 {
            if slot.is_none() {
                let client = self.connect().await?;
                if let Some(warning) = client.build_warning() {
                    // stdout занят протоколом MCP; stderr клиент складывает в журнал сервера.
                    eprintln!("bars-mcp: предупреждение: {warning}");
                }
                *slot = Some(client);
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
        description = "Похожие разделы по ID чанка из выдачи bars_rag_search (поле id). Перед правкой — \
                       найти другие места с тем же описанием; перед добавлением текста — проверить, \
                       нет ли его уже. Возвращает ближайшие чанки области modules без исходного; \
                       оценка — косинусная близость к исходному чанку. Близость не доказывает \
                       дублирование, согласие или зависимость документов: читай оба раздела. ID \
                       устаревает при изменении документа: при chunk_not_found найди раздел поиском \
                       заново. Без project — проект сессии."
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
