use super::*;

use std::sync::{Mutex, MutexGuard};

/// Все операции bars с конфигами сериализованы, включая обнаружение перед refresh.
static CLIENT_FILES: Mutex<()> = Mutex::new(());

pub(crate) fn lock() -> MutexGuard<'static, ()> {
    CLIENT_FILES
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

/// Один результат сохраняет успешные записи даже при ошибке следующего файла.
#[derive(Debug)]
pub struct RefreshResult {
    pub client: InstallClient,
    pub result: Result<Option<Installed>>,
}

pub fn refresh(env: &InstallEnv) -> Vec<RefreshResult> {
    [InstallClient::ClaudeCode, InstallClient::Codex]
        .into_iter()
        .map(|client| {
            let _guard = lock();
            let result = refresh_locked(env, client);
            RefreshResult { client, result }
        })
        .collect()
}

pub(super) fn refresh_locked(env: &InstallEnv, client: InstallClient) -> Result<Option<Installed>> {
    if !present(env, client)? {
        return Ok(None);
    }
    install_locked(env, client).map(Some)
}

fn present(env: &InstallEnv, client: InstallClient) -> Result<bool> {
    let paths = layout(env, client);
    // Обе проверки выполняются до любых записей; ошибка не означает отсутствие.
    let mcp = match client {
        InstallClient::ClaudeCode => json::has_mcp(&paths.mcp)?,
        InstallClient::Codex => codex_toml::has_mcp(&paths.mcp)?,
    };
    let hook = !json::hook_commands(&paths.hooks)?.is_empty();
    Ok(mcp || hook)
}

pub(crate) fn report(client: InstallClient, action: &str, result: &Result<Option<Installed>>) {
    let title = match client {
        InstallClient::ClaudeCode => "Claude Code",
        InstallClient::Codex => "Codex",
    };
    let done = match result {
        Ok(Some(done)) => Some(done),
        Err(ClientsError::Partial { done, .. }) => Some(done),
        _ => None,
    };
    if let Some(done) = done {
        for file in &done.changed_files {
            log::info!("{title}: {action}: {}", file.display());
        }
        if done.needs_hook_approval {
            log::warn!("Одобрите хук в Codex командой `/hooks`");
        }
    }
    if result.is_err() {
        log::error!("{title}: ошибка ({action}); проверьте настройки интеграции");
    }
}
