//! `bars-cli` как библиотека: разбор команд, вызов воркера и вывод (Т-20).
//!
//! Каждая команда — один вызов метода протокола ([`bars_protocol`]); ядра `bars-core` здесь нет.
//! Единственное обращение к диску — запись выбора корня перед `register` (Т-7).
//! Контракт — `docs/agent-tools/cli.md` и `docs/architecture/protocol.md`. Исполняемый файл
//! (`main.rs`) только разбирает аргументы и вызывает [`run`]; тесты вызывают [`run`] со своим
//! каналом и своим способом запуска воркера.

mod gate;
mod graph;
mod hook;
mod instruction;
mod render;
pub use graph::{GraphAction, GraphArgs};

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bars_protocol::{
    AutostartGetParams, AutostartSetParams, Client, ClientError, ClientKind, DuplicatesParams,
    EXIT_FAILURE, Endpoint, InstallClient, InstallParams, LAUNCH_DEADLINE, Method, OutlineParams,
    ProjectsParams, ReadParams, RegisterParams, ReindexParams, RootChoice, SearchParams,
    SimilarParams, StatusParams, Target, UninstallParams, UnregisterParams, connect_or_launch_with,
    prepare_project, spawn_worker, worker_exe,
};
use clap::{Args, Parser, Subcommand};

/// Аргументы командной строки.
#[derive(Debug, Parser)]
#[command(
    name = "bars-cli",
    version,
    about = "CLI для bars: поиск по документации проектов"
)]
pub struct Cli {
    /// Печатать ответ воркера как JSON, тот же, что у MCP-инструмента.
    #[arg(long, global = true)]
    pub json: bool,
    /// Проект: путь к его корню или имя. Без флага — ближайший зарегистрированный проект вверх от
    /// текущей папки.
    #[arg(long, global = true, value_name = "ПУТЬ")]
    pub project: Option<String>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Найти явные отношения с доказательствами.
    Relations(GraphArgs),
    /// Найти ограниченные пути между объектами.
    Paths(GraphArgs),
    /// Документированное влияние изменения roots.
    Impact(GraphArgs),
    /// Собрать источники задачи по ролям.
    Context(GraphArgs),
    /// Тематический обзор и аудит.
    Explore(GraphArgs),
    /// Просмотр и управление графом проекта.
    Graph {
        #[command(subcommand)]
        action: GraphAction,
    },
    /// Поиск по документации проекта.
    Search(SearchArgs),
    /// Похожие чанки по ID чанка из выдачи поиска.
    Similar(SimilarArgs),
    /// Аудит дублей проекта: точные, близкие, частичные и смысловые повторы.
    Duplicates(DuplicatesArgs),
    /// Структура документа: заголовки с номерами строк.
    Outline(OutlineArgs),
    /// Документ целиком или часть, с исходными номерами строк.
    Read {
        /// Путь от корня документации или от корня проекта.
        path: String,
        /// Диапазон N, N-M или N- (с единицы, включительно).
        #[arg(long, conflicts_with = "under")]
        lines: Option<String>,
        /// Полная цепочка заголовков; раздел с вложенными и все повторы.
        #[arg(long, conflicts_with = "lines")]
        under: Option<String>,
        /// Для пути папки: `desc` — обратный порядок по имени.
        #[arg(long, conflicts_with_all = ["lines", "under"])]
        order: Option<String>,
        /// Для пути папки: продолжение списка с предыдущей страницы.
        #[arg(long, conflicts_with_all = ["lines", "under"])]
        cursor: Option<String>,
    },
    /// Зарегистрированные проекты и их модули.
    Projects {
        /// Только записи реестра, в имени, составном имени (главный/путь) или пути которых есть
        /// этот текст (без учёта регистра, `\` равен `/`); подпроекты и worktree входят.
        /// Несовместим с `--project`.
        #[arg(long, value_name = "ТЕКСТ", conflicts_with = "project")]
        filter: Option<String>,
    },
    /// Состояние индекса проекта.
    Status,
    /// Переиндексация проекта.
    Reindex {
        /// Полная переиндексация вместо проверки актуальности.
        #[arg(long)]
        full: bool,
        /// Согласие на переиндексацию после смены модели (Т-25). Даёт только человек.
        #[arg(long)]
        accept_model_change: bool,
    },
    /// Зарегистрировать проект.
    Register {
        /// Папка проекта; по умолчанию — `--project`, иначе текущая.
        path: Option<PathBuf>,
        /// Корень документации нового проекта: `docs` — папка `docs/`, `all` — вся папка. Без
        /// флага: `docs/`, если она есть, иначе вся папка. Настроенный проект не меняется.
        #[arg(long, value_name = "docs|all", value_parser = parse_root)]
        root: Option<RootChoice>,
    },
    /// Снять проект с регистрации (файлы и индекс не удаляются).
    Unregister,
    /// Подключить bars к клиенту: MCP-сервер, хук начала сессии и skills.
    Install(InstallArgs),
    /// Убрать bars из клиента: только записи и skills bars.
    Uninstall {
        /// Клиент, из которого убрать bars.
        #[arg(long, value_enum)]
        client: ClientArg,
    },
    /// Автозапуск bars при входе в систему: состояние, включить, выключить.
    Autostart {
        #[arg(value_enum, default_value = "status")]
        action: AutostartAction,
    },
    /// Команды для хуков клиентов.
    Hook {
        #[command(subcommand)]
        event: HookEvent,
    },
}

#[derive(Debug, Subcommand)]
pub enum HookEvent {
    /// Хук начала сессии: читает JSON клиента из stdin и ставит проект на проверку.
    SessionStart,
    /// Хук перед вызовом инструмента: до первого вызова bars отклоняет чтение документации.
    PreToolUse {
        /// Папка документации проекта относительно его корня.
        #[arg(long, default_value = "docs")]
        docs_root: String,
    },
}

/// Что сделать с автозапуском.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum AutostartAction {
    Status,
    On,
    Off,
}

/// Клиент, в который ставится bars.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum ClientArg {
    ClaudeCode,
    Codex,
    KimiCode,
    Ouroboros,
    Iterion,
}

impl From<ClientArg> for InstallClient {
    fn from(client: ClientArg) -> Self {
        match client {
            ClientArg::ClaudeCode => Self::ClaudeCode,
            ClientArg::Codex => Self::Codex,
            ClientArg::KimiCode => Self::KimiCode,
            ClientArg::Ouroboros => Self::Ouroboros,
            ClientArg::Iterion => Self::Iterion,
        }
    }
}

#[derive(Debug, Args)]
pub struct InstallArgs {
    /// Клиент, в который поставить bars.
    #[arg(long, value_enum, required_unless_present = "print_instruction")]
    pub client: Option<ClientArg>,
    /// Только напечатать текстовую инструкцию по установке для агента.
    #[arg(long, conflicts_with = "client")]
    pub print_instruction: bool,
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Явный hybrid/mix/graph; без флага прежняя форма ответа.
    #[arg(long,value_parser=["hybrid","mix","graph"])]
    pub mode: Option<String>,
    /// В mix: отключить связанные пакеты и расширение кандидатов.
    #[arg(long, requires = "mode")]
    pub no_related: bool,
    /// В mix: ограничить размер пакета оснований в байтах.
    #[arg(long, requires = "mode", value_parser = clap::value_parser!(u32).range(1..))]
    pub related_budget: Option<u32>,
    /// Поисковый запрос.
    #[arg(required = true, num_args = 1..)]
    pub query: Vec<String>,
    /// Искать только в этом модуле (подпапке документации); `.` — корневой. Можно повторять.
    #[arg(long = "module", value_name = "МОДУЛЬ")]
    pub modules: Vec<String>,
    /// Искать только в этом файле, путь от корня документации. Можно повторять.
    #[arg(long = "file", value_name = "ФАЙЛ")]
    pub files: Vec<String>,
    /// Сколько результатов вернуть.
    #[arg(long)]
    pub limit: Option<u32>,
    /// Сколько кандидатов брать до реранка.
    #[arg(long)]
    pub candidates: Option<u32>,
    /// Порог опоры оценки результата (rerank, а без него косинус), 0…1.
    #[arg(long)]
    pub min_score: Option<f32>,
}

#[derive(Debug, Args)]
pub struct SimilarArgs {
    /// ID чанка из выдачи поиска (`id`), например `1842-9f3a1c07`.
    pub chunk_id: String,
    /// Брать кандидатов только из этого модуля; `.` — корневой. Можно повторять.
    #[arg(long = "module", value_name = "МОДУЛЬ")]
    pub modules: Vec<String>,
    /// Сколько похожих чанков вернуть.
    #[arg(long)]
    pub limit: Option<u32>,
}

#[derive(Debug, Args)]
pub struct DuplicatesArgs {
    /// Проверять только этот модуль; `.` — корневой. Можно повторять.
    #[arg(long = "module", value_name = "МОДУЛЬ")]
    pub modules: Vec<String>,
    /// Сколько записей вернуть.
    #[arg(long)]
    pub limit: Option<u32>,
}

#[derive(Debug, Args)]
pub struct OutlineArgs {
    /// Путь от корня документации, затем от корня проекта.
    pub path: String,
    /// Уровни заголовков через запятую, например `2,3`; по умолчанию все.
    #[arg(long, value_delimiter = ',', value_parser = clap::value_parser!(u8).range(1..=6))]
    pub levels: Vec<u8>,
    /// Только заголовки внутри этого: цепочка `Заголовок 1/Заголовок 2`.
    #[arg(long, conflicts_with = "under_line")]
    pub under: Option<String>,
    /// Только заголовки внутри заголовка на этой строке.
    #[arg(long)]
    pub under_line: Option<u32>,
}

/// Способ запустить воркер, когда к каналу не подключиться.
#[derive(Clone)]
pub enum Launch {
    /// `bars --background` рядом с exe клиента.
    Worker,
    /// Свой способ: для тестов.
    With(Arc<dyn Fn() -> io::Result<()> + Send + Sync>),
}

/// Окружение вызова: куда подключаться, откуда идёт команда и как запускать воркер.
#[derive(Clone)]
pub struct Env {
    pub endpoint: Endpoint,
    /// Рабочая папка клиента; по ней воркер находит проект (Т-15).
    pub cwd: PathBuf,
    pub launch: Launch,
    /// Stdin хука: JSON, который клиент передал команде `hook`. Читает `main`.
    pub hook_input: Option<String>,
}

impl Env {
    /// Окружение процесса: канал текущего пользователя, текущая папка, запуск `bars`.
    pub fn current() -> anyhow::Result<Self> {
        Ok(Self {
            endpoint: Endpoint::current()?,
            cwd: std::env::current_dir()?,
            launch: Launch::Worker,
            hook_input: None,
        })
    }
}

/// Выполняет команду, пишет результат в `out`, ошибку в `err`. Возвращает код выхода: `0`, `1`
/// (ошибка запроса), `2` (воркер недоступен), `3` (нужна переиндексация).
pub async fn run(cli: Cli, env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let json = cli.json;
    if let Command::Hook { event } = &cli.command {
        return match event {
            HookEvent::SessionStart => hook::session_start(env, err).await,
            HookEvent::PreToolUse { docs_root } => hook::pre_tool_use(env, docs_root, out, err),
        };
    }
    if let Command::Install(InstallArgs {
        print_instruction: true,
        ..
    }) = &cli.command
    {
        let text = instruction::text(std::env::current_exe().ok().as_deref());
        return match writeln!(out, "{text}") {
            Ok(()) => 0,
            Err(error) => {
                let _ = writeln!(err, "bars-cli: не удалось записать вывод: {error}");
                EXIT_FAILURE
            }
        };
    }
    match execute(cli, env, out, err).await {
        Ok(()) => 0,
        Err(Failure::Client(error)) => {
            let _ = writeln!(err, "{}", render::error(&error, json));
            error.exit_code()
        }
        Err(Failure::Output(error)) => {
            let _ = writeln!(err, "bars-cli: не удалось записать вывод: {error}");
            EXIT_FAILURE
        }
    }
}

enum Failure {
    Client(ClientError),
    Output(io::Error),
}

impl From<ClientError> for Failure {
    fn from(error: ClientError) -> Self {
        Self::Client(error)
    }
}

impl From<io::Error> for Failure {
    fn from(error: io::Error) -> Self {
        Self::Output(error)
    }
}

async fn connect(env: &Env) -> Result<Client, ClientError> {
    let launch = env.launch.clone();
    connect_or_launch_with(
        &env.endpoint,
        ClientKind::Cli,
        LAUNCH_DEADLINE,
        move || match launch {
            Launch::Worker => spawn_worker(&worker_exe()?),
            Launch::With(start) => start(),
        },
    )
    .await
}

fn project_arg(value: &str, cwd: &Path) -> String {
    // Путь — абсолютное значение, значение с точки или значение с разделителем, чей `cwd/значение` —
    // существующая папка. Остальное (в том числе составное имя `главный/путь`, которого нет в `cwd`)
    // уходит воркеру как есть и разбирается как имя (Т-49).
    let path = Path::new(value);
    if path.is_absolute()
        || value.starts_with('.')
        || (value.contains(['/', '\\']) && cwd.join(path).is_dir())
    {
        absolute(path, cwd)
    } else {
        value.to_string()
    }
}

/// Значение `--root` команды `register`: `docs` или `all`.
fn parse_root(value: &str) -> Result<RootChoice, String> {
    RootChoice::parse(value).ok_or_else(|| "допустимо `docs` или `all`".to_string())
}

/// Абсолютный путь от `cwd` без `.` и `..`: клиент не ходит на диск, проверяет путь воркер.
fn absolute(path: &Path, cwd: &Path) -> String {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let mut clean = PathBuf::new();
    for part in joined.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                clean.pop();
            }
            other => clean.push(other),
        }
    }
    clean.display().to_string()
}

/// Ответ метода: разобранный для человеческого вывода и исходный для `--json`.
struct Reply<T> {
    typed: T,
    raw: serde_json::Value,
}

fn emit<T>(
    out: &mut dyn Write,
    json: bool,
    reply: &Reply<T>,
    human: impl FnOnce(&T) -> String,
) -> Result<(), Failure> {
    let text = if json {
        // Ответ воркера как есть: поля, которых клиент не знает, не теряются.
        serde_json::to_string_pretty(&reply.raw).expect("ответ воркера сериализуется")
    } else {
        human(&reply.typed)
    };
    writeln!(out, "{text}")?;
    Ok(())
}

async fn call<M: Method>(client: &mut Client, params: &M) -> Result<Reply<M::Output>, Failure> {
    let raw = client.call_value(params, None).await?;
    let typed = serde_json::from_value(raw.clone())
        .map_err(|error| ClientError::BadReply(error.to_string()))?;
    Ok(Reply { typed, raw })
}

async fn execute(
    cli: Cli,
    env: &Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    let json = cli.json;
    let target = Target {
        cwd: env.cwd.display().to_string(),
        project: cli
            .project
            .as_deref()
            .map(|value| project_arg(value, &env.cwd)),
    };
    let mut client = connect(env).await?;
    if let Some(warning) = client.build_warning() {
        let _ = writeln!(err, "bars-cli: предупреждение: {warning}");
    }
    match cli.command {
        Command::Relations(args) => {
            graph::query(
                &mut client,
                target,
                json,
                out,
                args,
                bars_protocol::graph_types::Operation::Relations,
            )
            .await
        }
        Command::Paths(args) => {
            graph::query(
                &mut client,
                target,
                json,
                out,
                args,
                bars_protocol::graph_types::Operation::Paths,
            )
            .await
        }
        Command::Impact(args) => {
            graph::query(
                &mut client,
                target,
                json,
                out,
                args,
                bars_protocol::graph_types::Operation::Impact,
            )
            .await
        }
        Command::Context(args) => {
            graph::query(
                &mut client,
                target,
                json,
                out,
                args,
                bars_protocol::graph_types::Operation::Context,
            )
            .await
        }
        Command::Explore(args) => {
            graph::query(
                &mut client,
                target,
                json,
                out,
                args,
                bars_protocol::graph_types::Operation::Explore,
            )
            .await
        }
        Command::Graph { action } => graph::action(&mut client, target, json, out, action).await,
        Command::Search(args) => {
            let mode = args.mode;
            let params = SearchParams {
                diagnostic: false,
                target,
                query: args.query.join(" "),
                modules: args.modules,
                files: args.files,
                paths: Vec::new(),
                limit: args.limit,
                candidates: args.candidates,
                min_score: args.min_score,
                wait_models: None,
            };
            if let Some(mode) = mode {
                let mode = serde_json::from_value(serde_json::Value::String(mode))
                    .expect("clap проверил режим");
                let result = call(
                    &mut client,
                    &bars_protocol::GraphSearchParams {
                        text: !json,
                        search: params,
                        mode,
                        graph_budgets: Default::default(),
                        include_related: !args.no_related,
                        related_budget: args.related_budget,
                    },
                )
                .await?;
                return emit(
                    out,
                    json,
                    &result,
                    bars_protocol::graph_text::render_graph_search_text,
                );
            }
            emit(
                out,
                json,
                &call(&mut client, &params).await?,
                render::search,
            )
        }
        Command::Similar(args) => {
            let params = SimilarParams {
                target,
                chunk_id: args.chunk_id,
                modules: args.modules,
                paths: Vec::new(),
                exclude_document: false,
                limit: args.limit,
            };
            emit(
                out,
                json,
                &call(&mut client, &params).await?,
                render::similar,
            )
        }
        Command::Duplicates(args) => {
            let params = DuplicatesParams {
                target,
                modules: args.modules,
                limit: args.limit,
            };
            emit(
                out,
                json,
                &call(&mut client, &params).await?,
                render::duplicates,
            )
        }
        Command::Outline(args) => {
            let params = OutlineParams {
                target,
                path: args.path,
                levels: args.levels,
                under: args.under,
                under_line: args.under_line,
            };
            emit(
                out,
                json,
                &call(&mut client, &params).await?,
                render::outline,
            )
        }
        Command::Read {
            path,
            lines,
            under,
            order,
            cursor,
        } => {
            let params = ReadParams {
                target,
                path,
                lines,
                under,
                max_lines: None,
                order,
                cursor,
            };
            emit(out, json, &call(&mut client, &params).await?, render::read)
        }
        Command::Projects { filter } => {
            let params = ProjectsParams {
                target: None,
                filter: filter.clone(),
            };
            let reply = call(&mut client, &params).await?;
            let needle = bars_protocol::filter_text(filter.as_deref()).map(str::to_string);
            emit(out, json, &reply, |output| {
                render::projects_matching(output, needle.as_deref())
            })
        }
        Command::Status => {
            let params = StatusParams { target };
            emit(
                out,
                json,
                &call(&mut client, &params).await?,
                render::status,
            )
        }
        Command::Reindex {
            full,
            accept_model_change,
        } => {
            let params = ReindexParams {
                target,
                full,
                accept_model_change,
            };
            emit(
                out,
                json,
                &call(&mut client, &params).await?,
                render::reindex,
            )
        }
        Command::Register { path, root } => {
            let path = match (path, cli.project) {
                (Some(path), _) => absolute(&path, &env.cwd),
                (None, Some(project)) => project_arg(&project, &env.cwd),
                (None, None) => absolute(Path::new("."), &env.cwd),
            };
            // Выбор корня записывается до регистрации (Т-7); не папка — ошибку даст воркер.
            if Path::new(&path).is_dir() {
                prepare_project(Path::new(&path), root)?;
            }
            let params = RegisterParams { path };
            emit(
                out,
                json,
                &call(&mut client, &params).await?,
                render::register,
            )
        }
        Command::Install(args) => {
            let which = args
                .client
                .expect("клиент обязателен без --print-instruction");
            let params = InstallParams {
                client: which.into(),
            };
            emit(
                out,
                json,
                &call(&mut client, &params).await?,
                render::install,
            )
        }
        Command::Uninstall { client: which } => {
            let params = UninstallParams {
                client: which.into(),
            };
            emit(
                out,
                json,
                &call(&mut client, &params).await?,
                render::uninstall,
            )
        }
        Command::Autostart { action } => {
            let output = match action {
                AutostartAction::Status => call(&mut client, &AutostartGetParams {}).await?,
                AutostartAction::On => {
                    call(&mut client, &AutostartSetParams { enabled: true }).await?
                }
                AutostartAction::Off => {
                    call(&mut client, &AutostartSetParams { enabled: false }).await?
                }
            };
            emit(out, json, &output, render::autostart)
        }
        Command::Hook { .. } => unreachable!("hook разобран в run"),
        Command::Unregister => {
            let params = UnregisterParams { target };
            emit(
                out,
                json,
                &call(&mut client, &params).await?,
                render::unregister,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_prints_the_worker_reply_with_fields_the_client_does_not_know() {
        let reply = Reply {
            typed: 1_u32,
            raw: json!({ "known": 1, "unknown_to_client": { "layers": [1, 2] } }),
        };
        let mut out = Vec::new();
        emit(&mut out, true, &reply, |n| n.to_string()).unwrap_or_else(|_| panic!("запись"));
        let printed: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(printed, reply.raw);
    }

    #[test]
    fn text_mode_uses_the_typed_reply() {
        let reply = Reply {
            typed: 7_u32,
            raw: json!({ "known": 7 }),
        };
        let mut out = Vec::new();
        emit(&mut out, false, &reply, |n| format!("n={n}")).unwrap_or_else(|_| panic!("запись"));
        assert_eq!(String::from_utf8(out).unwrap(), "n=7\n");
    }
}

/// Разбор `project`: путь или имя (Т-49). Набор случаев один для `bars-mcp` и `bars-cli`.
#[cfg(test)]
mod project_arg_tests {
    use super::project_arg;
    use std::fs;
    use std::path::Path;

    #[test]
    fn qualified_name_without_a_folder_in_cwd_stays_a_name() {
        let cwd = tempfile::tempdir().unwrap();
        assert_eq!(
            project_arg("tmc-opovechenie/backend/api", cwd.path()),
            "tmc-opovechenie/backend/api"
        );
        assert_eq!(
            project_arg("tmc-opovechenie\backend\api", cwd.path()),
            "tmc-opovechenie\backend\api"
        );
        assert_eq!(project_arg("plain-name", cwd.path()), "plain-name");
    }

    #[test]
    fn existing_relative_folder_is_a_path() {
        let cwd = tempfile::tempdir().unwrap();
        fs::create_dir_all(cwd.path().join("backend/api")).unwrap();
        let got = project_arg("backend/api", cwd.path());
        assert_eq!(Path::new(&got), cwd.path().join("backend/api"));
    }

    #[test]
    fn dot_prefixed_value_is_a_path_even_if_missing() {
        let cwd = tempfile::tempdir().unwrap();
        let got = project_arg("./x", cwd.path());
        assert_eq!(Path::new(&got), cwd.path().join("x"));
        let dot = project_arg(".", cwd.path());
        assert_eq!(Path::new(&dot), cwd.path());
    }

    #[test]
    fn absolute_path_stays_absolute() {
        let cwd = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let got = project_arg(&other.path().display().to_string(), cwd.path());
        assert_eq!(Path::new(&got), other.path());
    }
}
