//! `bars-cli hook session-start`: хук начала сессии клиента (Т-6, `docs/features/agent-tools/cli.md`).
//!
//! Читает JSON клиента из stdin, берёт `cwd` и вызывает `session_start` воркера: корень проекта
//! находит и регистрирует воркер. Индексацию команда не ждёт. Сбой не должен ломать сессию
//! агента, поэтому выход всегда `0`, а причина идёт в stderr.

use std::io::Write;

use bars_protocol::SessionStartParams;
use serde::Deserialize;

use crate::{Env, connect};

#[derive(Debug, Default, Deserialize)]
struct HookInput {
    cwd: Option<String>,
    source: Option<String>,
}

pub async fn session_start(env: &Env, err: &mut dyn Write) -> i32 {
    if let Err(message) = try_session_start(env).await {
        let _ = writeln!(err, "bars-cli hook: {message}");
    }
    0
}

async fn try_session_start(env: &Env) -> Result<(), String> {
    let input: HookInput = match env.hook_input.as_deref().map(str::trim) {
        None | Some("") => HookInput::default(),
        Some(text) => serde_json::from_str(text)
            .map_err(|error| format!("вход хука не разобран как JSON: {error}"))?,
    };
    let params = SessionStartParams {
        cwd: input.cwd.unwrap_or_else(|| env.cwd.display().to_string()),
        source: input.source.unwrap_or_else(|| "startup".to_string()),
    };
    let mut client = connect(env).await.map_err(|error| error.to_string())?;
    client
        .call(&params)
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}
