//! Спайк: stdio-MCP-заглушка, которая при `initialize` запускает отвязанный дочерний процесс
//! (как bars-mcp запускает воркер). Дочерний процесс 15 минут пишет пульс в лог.
//! Если пульс продолжается после закрытия сессии агента — воркер переживает сессию.
//!
//!   spike-detach            — режим MCP-сервера (его запускает клиент)
//!   spike-detach child      — дочерний процесс

use anyhow::Result;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn log_path() -> PathBuf {
    let base = dirs::data_local_dir().unwrap_or_else(std::env::temp_dir);
    let dir = base.join("bars-spikes").join("detach");
    std::fs::create_dir_all(&dir).ok();
    dir.join("heartbeat.log")
}

fn log(msg: &str) {
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(log_path()) {
        let _ = writeln!(f, "{ts} pid={} {msg}", std::process::id());
    }
}

fn main() -> Result<()> {
    if std::env::args().nth(1).as_deref() == Some("child") {
        log("child started");
        for i in 0..450 {
            std::thread::sleep(Duration::from_secs(2));
            log(&format!("child heartbeat {i}"));
        }
        log("child finished");
        return Ok(());
    }
    log("mcp server started");
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let result = match method {
            "initialize" => {
                spawn_child();
                let proto = msg["params"]["protocolVersion"].clone();
                Some(json!({
                    "protocolVersion": proto,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "spike-detach", "version": "0.0.0"}
                }))
            }
            "tools/list" => Some(json!({"tools": [{
                "name": "detach_status",
                "description": "Spike: path to the heartbeat log of the detached child process",
                "inputSchema": {"type": "object", "properties": {}}
            }]})),
            "tools/call" => Some(json!({"content": [{"type": "text", "text": log_path().display().to_string()}]})),
            "ping" => Some(json!({})),
            _ => None,
        };
        if let (Some(id), Some(result)) = (id, result) {
            writeln!(out, "{}", json!({"jsonrpc": "2.0", "id": id, "result": result}))?;
            out.flush()?;
        }
    }
    log("mcp server stdin closed, exiting");
    Ok(())
}

fn spawn_child() {
    let exe = std::env::current_exe().unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        let base = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
        for (name, flags) in [("breakaway", base | CREATE_BREAKAWAY_FROM_JOB), ("no-breakaway", base)] {
            let r = Command::new(&exe)
                .arg("child")
                .creation_flags(flags)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            match r {
                Ok(c) => {
                    log(&format!("spawned child pid={} with {name}", c.id()));
                    return;
                }
                Err(e) => log(&format!("spawn with {name} failed: {e}")),
            }
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        match Command::new(&exe)
            .arg("child")
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(c) => log(&format!("spawned child pid={} in new process group", c.id())),
            Err(e) => log(&format!("spawn failed: {e}")),
        }
    }
}
