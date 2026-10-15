//! Установка bars в Claude Code и Codex и удаление оттуда (Т-21, Т-28, `docs/features/clients/clients.md`).
//!
//! Прописываются две записи `bars` — MCP-сервер и хук начала сессии — и skills. Чужие конфиги
//! правятся осторожно: копия `<файл>.bars-backup`, запись через временный файл и переименование,
//! только записи `bars`. Файл, который не разбирается, не меняется (ошибка). Ничего не пишется,
//! если файл и так в нужном виде: повторная установка идемпотентна.

mod codex_toml;
mod fsx;
mod json;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use bars_protocol::InstallClient;

use fsx::{remove_dir, write_atomic};

/// Имена skills bars: их ставит и убирает установка (Т-28).
const SKILLS: [(&str, &str); 2] = [
    (
        "bars-docs",
        include_str!("../../assets/skills/bars-docs/SKILL.md"),
    ),
    (
        "bars-session-log",
        include_str!("../../assets/skills/bars-session-log/SKILL.md"),
    ),
];

/// События хука, на которые срабатывает bars.
pub(crate) const HOOK_MATCHER: &str = "startup|resume";

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
}

/// Итог установки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub changed_files: Vec<PathBuf>,
    /// Хук нужно одобрить в самом клиенте (Codex, команда `/hooks`).
    pub needs_hook_approval: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ClientsError {
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
    let layout = layout(env, client);
    let mut changed = Vec::new();
    let mcp_changed = match client {
        InstallClient::ClaudeCode => json::install_mcp(&layout.mcp, &env.mcp_exe)?,
        InstallClient::Codex => codex_toml::install_mcp(&layout.mcp, &env.mcp_exe)?,
    };
    if mcp_changed {
        changed.push(layout.mcp.clone());
    }
    if json::install_hook(&layout.hooks, &env.hook_command())? {
        changed.push(layout.hooks.clone());
    }
    for (name, text) in SKILLS {
        let file = layout.skills.join(name).join("SKILL.md");
        if fsx::read_text(&file)?.as_deref() != Some(text) {
            write_atomic(&file, text, false)?;
            changed.push(file);
        }
    }
    Ok(Installed {
        changed_files: changed,
        // Codex не запускает новый или изменённый хук без одобрения; был ли он одобрен раньше,
        // bars не знает, поэтому напоминает при каждой установке.
        needs_hook_approval: client == InstallClient::Codex,
    })
}

/// Клиенты, в которых записи bars указывают не на текущие `bars-mcp` и `bars-cli`: приложение
/// переехало или обновилось, нужна переустановка. Читает файлы и ничего не пишет; не
/// установленный или нечитаемый клиент не считается устаревшим.
pub fn stale(env: &InstallEnv) -> Vec<InstallClient> {
    [InstallClient::ClaudeCode, InstallClient::Codex]
        .into_iter()
        .filter(|&client| is_stale(env, client).unwrap_or(false))
        .collect()
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
    let layout = layout(env, client);
    let mut changed = Vec::new();
    let mcp_changed = match client {
        InstallClient::ClaudeCode => json::uninstall_mcp(&layout.mcp)?,
        InstallClient::Codex => codex_toml::uninstall_mcp(&layout.mcp)?,
    };
    if mcp_changed {
        changed.push(layout.mcp.clone());
    }
    if json::uninstall_hook(&layout.hooks)? {
        changed.push(layout.hooks.clone());
    }
    for (name, _) in SKILLS {
        let dir = layout.skills.join(name);
        if remove_dir(&dir)? {
            changed.push(dir);
        }
    }
    Ok(changed)
}
