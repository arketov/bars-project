//! Приёмка IDEA-10: только временные профили, без обращения к моделям.
use super::*;
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::time::SystemTime;
use tempfile::TempDir;

fn env(home: &TempDir) -> InstallEnv {
    InstallEnv {
        home: home.path().to_path_buf(),
        mcp_exe: "/current/bars-mcp".into(),
        cli_exe: "/current/bars-cli".into(),
    }
}
fn write(file: &Path, text: &str) {
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
}
type Snapshot = BTreeMap<PathBuf, (Vec<u8>, SystemTime)>;
fn snapshot(root: &Path) -> Snapshot {
    fn visit(root: &Path, out: &mut Snapshot) {
        for entry in fs::read_dir(root).unwrap() {
            let file = entry.unwrap().path();
            if file.is_dir() {
                visit(&file, out);
            } else {
                out.insert(
                    file.clone(),
                    (
                        fs::read(&file).unwrap(),
                        fs::metadata(file).unwrap().modified().unwrap(),
                    ),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    visit(root, &mut out);
    out
}
fn updated(env: &InstallEnv, client: InstallClient) -> Installed {
    refresh(env)
        .into_iter()
        .find(|r| r.client == client)
        .unwrap()
        .result
        .unwrap()
        .unwrap()
}

#[test]
fn absent_clients_and_skills_only_are_never_installed() {
    let home = TempDir::new().unwrap();
    let env = env(&home);
    assert!(refresh(&env).iter().all(|r| matches!(r.result, Ok(None))));
    assert_eq!(fs::read_dir(home.path()).unwrap().count(), 0);
    write(
        &home.path().join(".claude.json"),
        r#"{"mcpServers":{"other":{"command":"other"}}}"#,
    );
    write(&home.path().join(".codex/config.toml"), "model = 'test'\n");
    write(
        &home.path().join(".kimi-code/config.toml"),
        "model = 'test'\n",
    );
    write(
        &home.path().join(".agents/skills/bars-docs/SKILL.md"),
        "custom",
    );
    write(
        &home.path().join(".claude/skills/bars-docs/SKILL.md"),
        "custom",
    );
    write(
        &home.path().join(".kimi-code/skills/bars-docs/SKILL.md"),
        "custom",
    );
    let before = snapshot(home.path());
    assert!(refresh(&env).iter().all(|r| matches!(r.result, Ok(None))));
    assert_eq!(snapshot(home.path()), before);
}

#[test]
fn mcp_or_hook_alone_repairs_all_clients() {
    for client in [
        InstallClient::ClaudeCode,
        InstallClient::Codex,
        InstallClient::KimiCode,
    ] {
        for hook_only in [false, true] {
            let home = TempDir::new().unwrap();
            let env = env(&home);
            let paths = layout(&env, client);
            if hook_only {
                let text = match client {
                    InstallClient::KimiCode => "[[hooks]]\nevent = \"SessionStart\"\nmatcher = \"old\"\ncommand = \"old/bars-cli hook session-start\"\n".to_string(),
                    _ => json!({"hooks":{"SessionStart":[{"matcher":"old", "hooks":[{"type":"prompt", "command":"bars-cli hook session-start"}]}]}}).to_string(),
                };
                write(&paths.hooks, &text);
            } else {
                write(
                    &paths.mcp,
                    if client == InstallClient::Codex {
                        "[mcp_servers.bars]\n"
                    } else {
                        r#"{"mcpServers":{"bars":{}}}"#
                    },
                );
            }
            let done = updated(&env, client);
            assert!(done.hook_changed);
            assert_eq!(done.needs_hook_approval, client == InstallClient::Codex);
            assert!(state(&env, client).installed);
            assert_eq!(
                hook_commands(client, &paths.hooks).unwrap(),
                [env.hook_command()]
            );
            let before = snapshot(home.path());
            let again = updated(&env, client);
            assert!(again.changed_files.is_empty());
            assert!(!again.needs_hook_approval);
            assert_eq!(snapshot(home.path()), before, "байты, mtime и backup");
        }
    }
}

#[test]
fn skills_and_mcp_updates_do_not_reset_codex_hook() {
    let home = TempDir::new().unwrap();
    let mut env = env(&home);
    install(&env, InstallClient::Codex).unwrap();
    let paths = layout(&env, InstallClient::Codex);
    let hook_before = fs::read(&paths.hooks).unwrap();
    let skill = paths.skills.join("bars-docs/SKILL.md");
    write(&skill, "user changes");
    env.mcp_exe = "/moved/bars-mcp".into();
    let done = updated(&env, InstallClient::Codex);
    assert_eq!(done.changed_files, [paths.mcp, skill]);
    assert!(!done.hook_changed && !done.needs_hook_approval);
    assert_eq!(fs::read(&paths.hooks).unwrap(), hook_before);
}

#[test]
fn normalizes_mixed_duplicate_hooks_preserving_foreign_groups() {
    let home = TempDir::new().unwrap();
    let env = env(&home);
    let paths = layout(&env, InstallClient::Codex);
    let foreign = json!({"type":"command", "command":"other", "timeout":42});
    write(&paths.hooks, &json!({"hooks":{"SessionStart":[
        {"matcher":"clear", "extra":7, "hooks":[foreign.clone(), {"type":"prompt","command":"old/bars-cli hook session-start"}]},
        {"matcher":"startup", "hooks":[{"command":"bars-cli hook session-start"}]}
    ], "Stop":[{"hooks":[foreign.clone()]}]}, "other":true}).to_string());
    let done = updated(&env, InstallClient::Codex);
    assert!(done.needs_hook_approval);
    let root: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&paths.hooks).unwrap()).unwrap();
    assert_eq!(root["hooks"]["SessionStart"].as_array().unwrap().len(), 2);
    assert_eq!(
        root["hooks"]["SessionStart"][0],
        json!({"matcher":"clear","extra":7,"hooks":[foreign.clone()]})
    );
    assert_eq!(root["hooks"]["Stop"][0]["hooks"][0], foreign);
    assert_eq!(
        json::hook_commands(&paths.hooks).unwrap(),
        [env.hook_command()]
    );
    // Правка чужого хука не делает bars устаревшим.
    let text = fs::read_to_string(&paths.hooks)
        .unwrap()
        .replace("other", "another");
    write(&paths.hooks, &text);
    assert!(!updated(&env, InstallClient::Codex).needs_hook_approval);
}

#[test]
fn invalid_config_preflight_prevents_partial_writes_and_continues_other_client() {
    for bad in [
        "{broken",
        r#"{"hooks":{"SessionStart":{}}}"#,
        r#"{"hooks":{"SessionStart":[{"hooks":3}]}}"#,
    ] {
        let home = TempDir::new().unwrap();
        let env = env(&home);
        write(
            &home.path().join(".claude.json"),
            r#"{"mcpServers":{"bars":{"command":"old"}}}"#,
        );
        write(&home.path().join(".claude/settings.json"), bad);
        write(
            &home.path().join(".codex/config.toml"),
            "[mcp_servers.bars]\ncommand='old'\n",
        );
        let before = fs::read(home.path().join(".claude.json")).unwrap();
        let outcomes = refresh(&env);
        assert!(outcomes[0].result.is_err());
        assert!(outcomes[1].result.as_ref().unwrap().is_some());
        assert!(
            outcomes[2].result.as_ref().unwrap().is_none(),
            "Kimi не установлен"
        );
        assert_eq!(fs::read(home.path().join(".claude.json")).unwrap(), before);
        assert!(!home.path().join(".claude.json.bars-backup").exists());
    }

    // Сломанный `config.toml` Kimi: ошибка только его, чужие файлы не пишутся.
    let home = TempDir::new().unwrap();
    let env = env(&home);
    write(
        &home.path().join(".kimi-code/mcp.json"),
        r#"{"mcpServers":{"bars":{"command":"old"}}}"#,
    );
    let kimi_config = home.path().join(".kimi-code/config.toml");
    write(&kimi_config, "[broken");
    let outcomes = refresh(&env);
    assert!(outcomes[0].result.as_ref().unwrap().is_none());
    assert!(outcomes[1].result.as_ref().unwrap().is_none());
    assert!(outcomes[2].result.is_err());
    assert_eq!(fs::read_to_string(&kimi_config).unwrap(), "[broken");
    assert!(!home.path().join(".kimi-code/mcp.json.bars-backup").exists());
}

#[test]
fn partial_write_report_survives_late_error_and_retry_finishes() {
    let home = TempDir::new().unwrap();
    let env = env(&home);
    let paths = layout(&env, InstallClient::Codex);
    write(&paths.mcp, "[mcp_servers.bars]\ncommand='old'\n");
    // Каталог вместо SKILL.md: одинаково воспроизводимый отказ чтения на всех ОС.
    let blocked = paths.skills.join("bars-docs/SKILL.md");
    fs::create_dir_all(&blocked).unwrap();
    let outcome = refresh(&env)
        .into_iter()
        .find(|r| r.client == InstallClient::Codex)
        .unwrap();
    let Err(ClientsError::Partial { done, .. }) = outcome.result else {
        panic!("нужен частичный отчёт");
    };
    assert_eq!(done.changed_files, [paths.mcp, paths.hooks]);
    assert!(done.needs_hook_approval);
    fs::remove_dir(&blocked).unwrap();
    let done = updated(&env, InstallClient::Codex);
    assert_eq!(done.changed_files.len(), 3);
    assert!(!done.needs_hook_approval);
}

#[test]
fn config_conflict_rereads_foreign_edits_and_bounds_retries() {
    let home = TempDir::new().unwrap();
    let file = home.path().join("config");
    write(&file, "initial");
    let mut attempts = 0;
    fsx::update_text(&file, |before| {
        attempts += 1;
        if attempts == 1 {
            write(&file, "foreign");
        }
        Ok(Some(format!("{}+bars", before.unwrap())))
    })
    .unwrap();
    assert_eq!(attempts, 2);
    assert_eq!(fs::read_to_string(&file).unwrap(), "foreign+bars");
    let error = fsx::update_text(&file, |_| {
        attempts += 1;
        write(&file, &attempts.to_string());
        Ok(Some("bars".into()))
    })
    .unwrap_err();
    assert!(matches!(error, ClientsError::Conflict { .. }));
    assert_eq!(attempts, 5);
    assert!(!home.path().join("config.bars-tmp").exists());
}

#[test]
fn uninstall_and_refresh_share_the_lock_in_both_orders() {
    for uninstall_first in [false, true] {
        let home = TempDir::new().unwrap();
        let env = env(&home);
        install(&env, InstallClient::Codex).unwrap();
        let guard = lock();
        let (started, wait) = std::sync::mpsc::channel();
        let other_env = env.clone();
        let task = std::thread::spawn(move || {
            started.send(()).unwrap();
            if uninstall_first {
                refresh(&other_env);
            } else {
                uninstall(&other_env, InstallClient::Codex).unwrap();
            }
        });
        wait.recv().unwrap();
        if uninstall_first {
            uninstall_locked(&env, InstallClient::Codex).unwrap();
        } else {
            refresh::refresh_locked(&env, InstallClient::Codex).unwrap();
        }
        drop(guard);
        task.join().unwrap();
        assert!(refresh(&env).iter().all(|r| matches!(r.result, Ok(None))));
        assert!(!state(&env, InstallClient::Codex).installed);
    }
}
