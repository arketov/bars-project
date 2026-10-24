//! Установка bars в Claude Code и Codex и удаление оттуда (Т-21, Т-28, `docs/features/clients/clients.md`).
//!
//! Прописываются две записи `bars` — MCP-сервер и хук начала сессии — и skills. Чужие конфиги
//! правятся осторожно: копия `<файл>.bars-backup`, запись через временный файл и переименование,
//! только записи `bars`. Файл, который не разбирается, не меняется (ошибка). Ничего не пишется,
//! если файл и так в нужном виде: повторная установка идемпотентна.

mod codex_toml;
mod fsx;
mod json;
mod project_gate;
mod refresh;

#[cfg(test)]
mod refresh_tests;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use bars_protocol::InstallClient;

use fsx::{remove_dir, write_atomic};
pub use project_gate::{GateProject, sync as sync_project_gates};
pub use refresh::{RefreshResult, refresh};
pub(crate) use refresh::{lock, report, report_gates};

/// Имена skills bars: их ставит и убирает установка (Т-28).
const SKILLS: [(&str, &str); 3] = [
    (
        "bars-docs",
        include_str!("../../assets/skills/bars-docs/SKILL.md"),
    ),
    (
        "bars-session-log",
        include_str!("../../assets/skills/bars-session-log/SKILL.md"),
    ),
    (
        "bars-end",
        include_str!("../../assets/skills/bars-end/SKILL.md"),
    ),
];

/// События хука, на которые срабатывает bars.
pub(crate) const HOOK_MATCHER: &str = "startup|resume";

/// Инструменты, перед которыми срабатывает гейт чтения документации.
pub(crate) const GATE_MATCHER: &str = "Read|Grep|Glob|Bash|PowerShell|mcp__bars__.*";

/// Откуда брать файлы клиентов и куда указывают записи bars.
#[derive(Debug, Clone)]
pub struct InstallEnv {
    /// Домашняя папка пользователя: в ней `.claude.json`, `.claude/`, `.codex/`, `.agents/`.
    pub home: PathBuf,
    /// Абсолютный путь к `bars-mcp`.
    pub mcp_exe: PathBuf,
    /// Абсолютный путь к `bars-cli`.
    pub cli_exe: PathBuf,
}

impl InstallEnv {
    /// Пути от расположения exe воркера: `bars-mcp` и `bars-cli` лежат рядом с ним.
    pub fn detect() -> Option<Self> {
        let exe = std::env::current_exe().ok()?;
        let dir = exe.parent()?;
        let name = |base: &str| dir.join(format!("{base}{}", std::env::consts::EXE_SUFFIX));
        Some(Self {
            home: bars_protocol::user_home()?,
            mcp_exe: name("bars-mcp"),
            cli_exe: name("bars-cli"),
        })
    }

    /// Команда хука: `"<bars-cli>" hook session-start`.
    pub fn hook_command(&self) -> String {
        format!("\"{}\" hook session-start", self.cli_exe.display())
    }

    /// Команда гейта проекта: `"<bars-cli>" hook pre-tool-use --docs-root "<папка>"`.
    pub fn gate_command(&self, docs_root: &str) -> String {
        format!(
            "\"{}\" hook pre-tool-use --docs-root \"{docs_root}\"",
            self.cli_exe.display()
        )
    }
}

/// Итог установки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub changed_files: Vec<PathBuf>,
    /// Хук нужно одобрить в самом клиенте (Codex, команда `/hooks`).
    pub needs_hook_approval: bool,
    pub hook_changed: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ClientsError {
    #[error("{file}: конкурентная правка, обновление отложено до следующего запуска")]
    Conflict { file: PathBuf },
    #[error("{source}")]
    Partial {
        done: Installed,
        #[source]
        source: Box<ClientsError>,
    },
    /// Файл клиента не разбирается: bars ничего не пишет.
    #[error("{file} не разбирается ({message}): bars его не менял")]
    Parse { file: PathBuf, message: String },
    #[error("{file}: {source}")]
    Io {
        file: PathBuf,
        source: std::io::Error,
    },
}

type Result<T> = std::result::Result<T, ClientsError>;

fn io(file: &Path) -> impl FnOnce(std::io::Error) -> ClientsError {
    let file = file.to_path_buf();
    move |source| ClientsError::Io { file, source }
}

/// Файлы клиента: где его MCP-запись, где хук, где skills.
struct Layout {
    /// Файл с MCP-записью.
    mcp: PathBuf,
    /// Файл с хуком (формат JSON с `hooks.SessionStart`).
    hooks: PathBuf,
    skills: PathBuf,
}

fn layout(env: &InstallEnv, client: InstallClient) -> Layout {
    match client {
        InstallClient::ClaudeCode => Layout {
            mcp: env.home.join(".claude.json"),
            hooks: env.home.join(".claude").join("settings.json"),
            skills: env.home.join(".claude").join("skills"),
        },
        // Skills Codex читает из `~/.agents/skills` (learn.chatgpt.com/docs/build-skills).
        InstallClient::Codex => Layout {
            mcp: env.home.join(".codex").join("config.toml"),
            hooks: env.home.join(".codex").join("hooks.json"),
            skills: env.home.join(".agents").join("skills"),
        },
    }
}

/// Подключает bars к клиенту: MCP-сервер, хук и skills.
pub fn install(env: &InstallEnv, client: InstallClient) -> Result<Installed> {
    let _guard = lock();
    install_locked(env, client)
}

fn install_locked(env: &InstallEnv, client: InstallClient) -> Result<Installed> {
    let layout = layout(env, client);
    let mut done = Installed {
        changed_files: Vec::new(),
        needs_hook_approval: false,
        hook_changed: false,
    };
    let result = (|| {
        // Полный разбор обоих конфигов до первой записи.
        match client {
            InstallClient::ClaudeCode => json::validate_mcp(&layout.mcp, &env.mcp_exe)?,
            InstallClient::Codex => codex_toml::validate_mcp(&layout.mcp, &env.mcp_exe)?,
        }
        json::validate_hook(&layout.hooks, &env.hook_command())?;
        let mcp_changed = match client {
            InstallClient::ClaudeCode => json::install_mcp(&layout.mcp, &env.mcp_exe)?,
            InstallClient::Codex => codex_toml::install_mcp(&layout.mcp, &env.mcp_exe)?,
        };
        if mcp_changed {
            done.changed_files.push(layout.mcp.clone());
        }
        done.hook_changed = json::install_hook(&layout.hooks, &env.hook_command())?;
        done.needs_hook_approval = client == InstallClient::Codex && done.hook_changed;
        if done.hook_changed {
            done.changed_files.push(layout.hooks.clone());
        }
        for (name, text) in SKILLS {
            let file = layout.skills.join(name).join("SKILL.md");
            if fsx::read_text(&file)?.as_deref() != Some(text) {
                write_atomic(&file, text, false)?;
                done.changed_files.push(file);
            }
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(done),
        Err(error) if done.changed_files.is_empty() => Err(error),
        Err(error) => Err(ClientsError::Partial {
            done,
            source: Box::new(error),
        }),
    }
}

fn is_stale(env: &InstallEnv, client: InstallClient) -> Result<bool> {
    let layout = layout(env, client);
    let mcp = match client {
        InstallClient::ClaudeCode => json::mcp_command(&layout.mcp)?,
        InstallClient::Codex => codex_toml::mcp_command(&layout.mcp)?,
    };
    let mcp_stale = mcp.is_some_and(|command| command != env.mcp_exe.display().to_string());
    let hook = env.hook_command();
    let hook_stale = json::hook_commands(&layout.hooks)?
        .iter()
        .any(|command| *command != hook);
    Ok(mcp_stale || hook_stale)
}

/// Состояние установки bars в клиенте: то, что показывает вкладка «Клиенты».
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientState {
    /// В конфиге клиента есть MCP-запись bars.
    pub installed: bool,
    /// Записи указывают не на текущие `bars-mcp` и `bars-cli`.
    pub stale: bool,
    /// Файлы, которые bars правит у этого клиента.
    pub files: Vec<PathBuf>,
}

/// Читает конфиги клиента и ничего не пишет. Файл, который не разбирается, даёт «не установлен»:
/// установка в него закончится ошибкой с именем файла.
pub fn state(env: &InstallEnv, client: InstallClient) -> ClientState {
    let layout = layout(env, client);
    let mcp = match client {
        InstallClient::ClaudeCode => json::mcp_command(&layout.mcp),
        InstallClient::Codex => codex_toml::mcp_command(&layout.mcp),
    };
    ClientState {
        installed: matches!(mcp, Ok(Some(_))),
        stale: is_stale(env, client).unwrap_or(false),
        files: vec![layout.mcp, layout.hooks, layout.skills],
    }
}

/// Убирает из клиента записи и skills bars; чужие записи не трогает.
pub fn uninstall(env: &InstallEnv, client: InstallClient) -> Result<Vec<PathBuf>> {
    let _guard = lock();
    uninstall_locked(env, client)
}

fn uninstall_locked(env: &InstallEnv, client: InstallClient) -> Result<Vec<PathBuf>> {
    let layout = layout(env, client);
    let mut done = Installed {
        changed_files: Vec::new(),
        hook_changed: false,
        needs_hook_approval: false,
    };
    let result = (|| {
        // Не удалять MCP перед обнаружением неразбираемого hooks-файла.
        match client {
            InstallClient::ClaudeCode => {
                json::has_mcp(&layout.mcp)?;
            }
            InstallClient::Codex => {
                codex_toml::has_mcp(&layout.mcp)?;
            }
        }
        json::hook_commands(&layout.hooks)?;
        let mcp_changed = match client {
            InstallClient::ClaudeCode => json::uninstall_mcp(&layout.mcp)?,
            InstallClient::Codex => codex_toml::uninstall_mcp(&layout.mcp)?,
        };
        if mcp_changed {
            done.changed_files.push(layout.mcp.clone());
        }
        if json::uninstall_hook(&layout.hooks)? {
            done.changed_files.push(layout.hooks.clone());
        }
        if client == InstallClient::ClaudeCode {
            done.changed_files.extend(project_gate::remove_all(env));
        }
        for (name, _) in SKILLS {
            let dir = layout.skills.join(name);
            if remove_dir(&dir)? {
                done.changed_files.push(dir);
            }
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(done.changed_files),
        Err(error) if done.changed_files.is_empty() => Err(error),
        Err(error) => Err(ClientsError::Partial {
            done,
            source: Box::new(error),
        }),
    }
}
