//! `bars-cli` как библиотека: разбор команд, вызов воркера и вывод (Т-20).
//!
//! Каждая команда — один вызов метода протокола ([`bars_protocol`]); ядра `bars-core` здесь нет.
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
    ProjectsParams, ReadParams, RegisterParams, ReindexParams, SearchParams, SimilarParams,
    StatusParams, Target, UninstallParams, UnregisterParams, connect_or_launch_with, spawn_worker,
    worker_exe,
};
use clap::{Args, Parser, Subcommand};
use serde::Serialize;

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
    /// Документ целиком, с номерами строк.
    Read {
        /// Путь от корня документации или от корня проекта.
        path: String,
    },
    /// Зарегистрированные проекты и их модули.
    Projects,
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
}

impl From<ClientArg> for InstallClient {
    fn from(client: ClientArg) -> Self {
        match client {
            ClientArg::ClaudeCode => Self::ClaudeCode,
            ClientArg::Codex => Self::Codex,
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
    /// Порог оценки результата.
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
    match execute(cli, env, out).await {
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

/// Значение `--project`: путь (с разделителем или точкой в начале) приводится к абсолютному
/// от рабочей папки, остальное — имя проекта, его разберёт воркер.
fn project_arg(value: &str, cwd: &Path) -> String {
    if value.starts_with('.') || value.contains(['/', '\\']) {
        absolute(Path::new(value), cwd)
    } else {
        value.to_string()
    }
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

fn emit<T: Serialize>(
    out: &mut dyn Write,
    json: bool,
    output: &T,
    human: impl FnOnce(&T) -> String,
) -> Result<(), Failure> {
    let text = if json {
        serde_json::to_string_pretty(output).expect("ответы методов сериализуются")
    } else {
        human(output)
    };
    writeln!(out, "{text}")?;
    Ok(())
}

async fn call<M: Method>(client: &mut Client, params: &M) -> Result<M::Output, Failure> {
    Ok(client.call(params).await?)
}

async fn execute(cli: Cli, env: &Env, out: &mut dyn Write) -> Result<(), Failure> {
    let json = cli.json;
    let target = Target {
        cwd: env.cwd.display().to_string(),
        project: cli
            .project
            .as_deref()
            .map(|value| project_arg(value, &env.cwd)),
    };
    let mut client = connect(env).await?;
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
                target,
                query: args.query.join(" "),
                modules: args.modules,
                files: args.files,
                limit: args.limit,
                candidates: args.candidates,
                min_score: args.min_score,
            };
            if let Some(mode) = mode {
                let mode = serde_json::from_value(serde_json::Value::String(mode))
                    .expect("clap проверил режим");
                let result = call(
                    &mut client,
                    &bars_protocol::GraphSearchParams {
                        search: params,
                        mode,
                        graph_budgets: Default::default(),
                        include_related: !args.no_related,
                        related_budget: args.related_budget,
                    },
                )
                .await?;
                return emit(out, json, &result, |r| {
                    serde_json::to_string_pretty(r).expect("DTO сериализуется")
                });
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
        Command::Read { path } => {
            let params = ReadParams { target, path };
            emit(out, json, &call(&mut client, &params).await?, render::read)
        }
        Command::Projects => emit(
            out,
            json,
            &call(&mut client, &ProjectsParams {}).await?,
            render::projects,
        ),
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
        Command::Register { path } => {
            let path = match (path, cli.project) {
                (Some(path), _) => absolute(&path, &env.cwd),
                (None, Some(project)) => project_arg(&project, &env.cwd),
                (None, None) => absolute(Path::new("."), &env.cwd),
            };
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
