//! Тесты установки на временных копиях конфигов; настоящие `~/.claude` и `~/.codex` не трогаются.

use std::fs;
use std::path::{Path, PathBuf};

use bars_protocol::InstallClient;
use serde_json::{Value, json};
use tempfile::TempDir;

use super::{ClientsError, InstallEnv, install, uninstall};

fn env(home: &TempDir) -> InstallEnv {
    InstallEnv {
        home: home.path().to_path_buf(),
        mcp_exe: PathBuf::from("/opt/bars/bars-mcp"),
        cli_exe: PathBuf::from("/opt/bars/bars-cli"),
    }
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[test]
fn claude_install_writes_mcp_hook_and_skills_keeping_foreign_entries() {
    let home = TempDir::new().unwrap();
    let env = env(&home);
    let claude_json = home.path().join(".claude.json");
    let settings = home.path().join(".claude/settings.json");
    write(
        &claude_json,
        r#"{"zeta": 1, "mcpServers": {"serena": {"command": "serena"}}, "alpha": 2}"#,
    );
    write(
        &settings,
        r#"{"hooks": {"SessionStart": [{"matcher": "startup", "hooks": [{"type": "command", "command": "serena-hooks activate"}]}]}}"#,
    );

    let done = install(&env, InstallClient::ClaudeCode).unwrap();
    assert!(!done.needs_hook_approval);
    assert_eq!(done.changed_files.len(), 5, "{:?}", done.changed_files);

    let value = read_json(&claude_json);
    assert_eq!(value["mcpServers"]["serena"]["command"], "serena");
    assert_eq!(value["mcpServers"]["bars"]["command"], "/opt/bars/bars-mcp");
    assert_eq!(value["mcpServers"]["bars"]["type"], "stdio");
    let keys: Vec<_> = value.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["zeta", "mcpServers", "alpha"], "порядок ключей");
    let backup = fs::read_to_string(home.path().join(".claude.json.bars-backup")).unwrap();
    assert!(backup.contains("serena") && !backup.contains("bars-mcp"));

    let hooks = &read_json(&settings)["hooks"]["SessionStart"];
    assert_eq!(hooks.as_array().unwrap().len(), 2);
    assert_eq!(hooks[0]["hooks"][0]["command"], "serena-hooks activate");
    assert_eq!(hooks[1]["matcher"], "startup|resume");
    assert_eq!(
        hooks[1]["hooks"][0]["command"],
        "\"/opt/bars/bars-cli\" hook session-start"
    );

    for name in ["bars-docs", "bars-session-log", "bars-end"] {
        let skill = home
            .path()
            .join(".claude/skills")
            .join(name)
            .join("SKILL.md");
        assert!(
            fs::read_to_string(skill)
                .unwrap()
                .contains(&format!("name: {name}"))
        );
    }
}

#[test]
fn install_is_idempotent_and_updates_moved_paths() {
    let home = TempDir::new().unwrap();
    let mut env = env(&home);
    install(&env, InstallClient::ClaudeCode).unwrap();
    let settings = home.path().join(".claude/settings.json");
    let first = fs::read_to_string(&settings).unwrap();

    let again = install(&env, InstallClient::ClaudeCode).unwrap();
    assert!(again.changed_files.is_empty(), "{:?}", again.changed_files);
    assert_eq!(fs::read_to_string(&settings).unwrap(), first);

    env.mcp_exe = PathBuf::from("/new/bars-mcp");
    env.cli_exe = PathBuf::from("/new/bars-cli");
    let moved = install(&env, InstallClient::ClaudeCode).unwrap();
    assert_eq!(moved.changed_files.len(), 2);
    let hooks = &read_json(&settings)["hooks"]["SessionStart"];
    assert_eq!(hooks.as_array().unwrap().len(), 1, "дублей нет");
    assert_eq!(
        hooks[0]["hooks"][0]["command"],
        "\"/new/bars-cli\" hook session-start"
    );
}

#[test]
fn claude_uninstall_removes_only_bars() {
    let home = TempDir::new().unwrap();
    let env = env(&home);
    let claude_json = home.path().join(".claude.json");
    let settings = home.path().join(".claude/settings.json");
    write(
        &claude_json,
        r#"{"mcpServers": {"serena": {"command": "serena"}}}"#,
    );
    write(
        &settings,
        r#"{"model": "x", "hooks": {"SessionStart": [{"hooks": [{"type": "command", "command": "other"}]}]}}"#,
    );
    write(
        &home.path().join(".claude/skills/custom/SKILL.md"),
        "чужой skill",
    );
    install(&env, InstallClient::ClaudeCode).unwrap();

    let removed = uninstall(&env, InstallClient::ClaudeCode).unwrap();
    assert_eq!(removed.len(), 5);
    assert_eq!(
        read_json(&claude_json),
        json!({"mcpServers": {"serena": {"command": "serena"}}})
    );
    assert_eq!(
        read_json(&settings),
        json!({"model": "x", "hooks": {"SessionStart": [{"hooks": [{"type": "command", "command": "other"}]}]}})
    );
    assert!(home.path().join(".claude/skills/custom/SKILL.md").exists());
    assert!(!home.path().join(".claude/skills/bars-docs").exists());
    assert!(!home.path().join(".claude/skills/bars-end").exists());
    assert!(
        uninstall(&env, InstallClient::ClaudeCode)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn uninstall_does_not_create_files() {
    let home = TempDir::new().unwrap();
    let env = env(&home);
    assert!(uninstall(&env, InstallClient::Codex).unwrap().is_empty());
    assert_eq!(fs::read_dir(home.path()).unwrap().count(), 0);
}

#[test]
fn codex_install_edits_toml_keeping_comments_and_asks_hook_approval() {
    let home = TempDir::new().unwrap();
    let env = env(&home);
    let config = home.path().join(".codex/config.toml");
    write(
        &config,
        "# мои настройки\nmodel = \"gpt\"\n\n[mcp_servers.other]\ncommand = \"other\"\n",
    );

    let done = install(&env, InstallClient::Codex).unwrap();
    assert!(done.needs_hook_approval);
    let text = fs::read_to_string(&config).unwrap();
    assert!(
        text.starts_with("# мои настройки\nmodel = \"gpt\""),
        "{text}"
    );
    let parsed: toml::Table = text.parse().unwrap();
    assert_eq!(
        parsed["mcp_servers"]["other"]["command"].as_str(),
        Some("other")
    );
    assert_eq!(
        parsed["mcp_servers"]["bars"]["command"].as_str(),
        Some("/opt/bars/bars-mcp")
    );
    assert!(home.path().join(".codex/config.toml.bars-backup").exists());
    let hooks = read_json(&home.path().join(".codex/hooks.json"));
    assert_eq!(
        hooks["hooks"]["SessionStart"][0]["matcher"],
        "startup|resume"
    );
    assert!(
        home.path()
            .join(".agents/skills/bars-docs/SKILL.md")
            .exists()
    );

    let again = install(&env, InstallClient::Codex).unwrap();
    assert!(again.changed_files.is_empty());
    assert_eq!(fs::read_to_string(&config).unwrap(), text);

    uninstall(&env, InstallClient::Codex).unwrap();
    let after: toml::Table = fs::read_to_string(&config).unwrap().parse().unwrap();
    assert!(after["mcp_servers"].get("bars").is_none());
    assert!(after["mcp_servers"].get("other").is_some());
    assert!(!home.path().join(".agents/skills/bars-docs").exists());
}

#[test]
fn unparsable_file_is_not_written() {
    let home = TempDir::new().unwrap();
    let env = env(&home);
    let claude_json = home.path().join(".claude.json");
    write(&claude_json, "{ это не json");
    let error = install(&env, InstallClient::ClaudeCode).unwrap_err();
    assert!(matches!(error, ClientsError::Parse { .. }));
    assert_eq!(fs::read_to_string(&claude_json).unwrap(), "{ это не json");
    assert!(!home.path().join(".claude.json.bars-backup").exists());

    let config = home.path().join(".codex/config.toml");
    write(&config, "[broken");
    assert!(matches!(
        install(&env, InstallClient::Codex).unwrap_err(),
        ClientsError::Parse { .. }
    ));
    assert_eq!(fs::read_to_string(&config).unwrap(), "[broken");

    let kimi_config = home.path().join(".kimi-code/config.toml");
    write(&kimi_config, "[broken");
    assert!(matches!(
        install(&env, InstallClient::KimiCode).unwrap_err(),
        ClientsError::Parse { .. }
    ));
    assert_eq!(fs::read_to_string(&kimi_config).unwrap(), "[broken");
    assert!(!home.path().join(".kimi-code/mcp.json").exists());
}

#[test]
fn stale_reports_only_installed_clients_with_old_paths() {
    let stale = |env: &InstallEnv| {
        [
            InstallClient::ClaudeCode,
            InstallClient::Codex,
            InstallClient::KimiCode,
        ]
        .into_iter()
        .filter(|&client| super::state(env, client).stale)
        .collect::<Vec<_>>()
    };
    let home = TempDir::new().unwrap();
    let mut env = env(&home);
    assert!(stale(&env).is_empty(), "ничего не установлено");
    install(&env, InstallClient::ClaudeCode).unwrap();
    install(&env, InstallClient::Codex).unwrap();
    install(&env, InstallClient::KimiCode).unwrap();
    assert!(stale(&env).is_empty());

    env.cli_exe = PathBuf::from("/new/bars-cli");
    assert_eq!(
        stale(&env),
        [
            InstallClient::ClaudeCode,
            InstallClient::Codex,
            InstallClient::KimiCode
        ]
    );
    install(&env, InstallClient::ClaudeCode).unwrap();
    install(&env, InstallClient::KimiCode).unwrap();
    assert_eq!(stale(&env), [InstallClient::Codex]);
}

#[test]
fn kimi_install_edits_mcp_json_and_config_toml_keeping_everything_else() {
    let home = TempDir::new().unwrap();
    let env = env(&home);
    let mcp = home.path().join(".kimi-code/mcp.json");
    let config = home.path().join(".kimi-code/config.toml");
    write(
        &config,
        "# мои настройки\ndefault_model = \"k3\"\n\n[providers.kimi]\ntype = \"kimi\"\napi_key = \"secret\"\n\n[[permission.rules]]\ndecision = \"allow\"\npattern = \"Read\"\n\n[[hooks]]\nevent = \"Notification\"\nmatcher = \"task\\\\.completed\"\ncommand = \"notify-send done\"\n",
    );

    let done = install(&env, InstallClient::KimiCode).unwrap();
    assert!(!done.needs_hook_approval);
    assert_eq!(done.changed_files.len(), 5, "{:?}", done.changed_files);

    let value = read_json(&mcp);
    assert_eq!(value["mcpServers"]["bars"]["command"], "/opt/bars/bars-mcp");
    assert_eq!(value["mcpServers"]["bars"]["type"], "stdio");
    assert_eq!(value["mcpServers"]["bars"]["args"], json!([]));

    let text = fs::read_to_string(&config).unwrap();
    assert!(
        text.starts_with("# мои настройки\ndefault_model = \"k3\""),
        "{text}"
    );
    assert!(text.contains("api_key = \"secret\""), "{text}");
    let parsed: toml::Table = text.parse().unwrap();
    assert_eq!(parsed["default_model"].as_str(), Some("k3"));
    assert_eq!(
        parsed["permission"]["rules"][0]["pattern"].as_str(),
        Some("Read")
    );
    let hooks = parsed["hooks"].as_array().unwrap();
    assert_eq!(hooks.len(), 2, "чужой хук сохранён, bars добавлен");
    assert_eq!(hooks[0]["event"].as_str(), Some("Notification"));
    assert_eq!(hooks[0]["command"].as_str(), Some("notify-send done"));
    assert_eq!(hooks[1]["event"].as_str(), Some("SessionStart"));
    assert_eq!(hooks[1]["matcher"].as_str(), Some("startup|resume"));
    assert_eq!(
        hooks[1]["command"].as_str(),
        Some("\"/opt/bars/bars-cli\" hook session-start")
    );
    assert!(
        !hooks[1].as_table().unwrap().contains_key("timeout"),
        "только разрешённые Kimi поля"
    );
    assert!(
        home.path()
            .join(".kimi-code/config.toml.bars-backup")
            .exists()
    );
    for name in ["bars-docs", "bars-session-log", "bars-end"] {
        assert!(
            home.path()
                .join(".kimi-code/skills")
                .join(name)
                .join("SKILL.md")
                .exists()
        );
    }
}

#[test]
fn kimi_install_is_idempotent_and_updates_moved_paths_without_duplicates() {
    let home = TempDir::new().unwrap();
    let mut env = env(&home);
    install(&env, InstallClient::KimiCode).unwrap();
    let config = home.path().join(".kimi-code/config.toml");
    let first = fs::read_to_string(&config).unwrap();

    let again = install(&env, InstallClient::KimiCode).unwrap();
    assert!(again.changed_files.is_empty(), "{:?}", again.changed_files);
    assert_eq!(fs::read_to_string(&config).unwrap(), first);

    env.mcp_exe = PathBuf::from("/new/bars-mcp");
    env.cli_exe = PathBuf::from("/new/bars-cli");
    let moved = install(&env, InstallClient::KimiCode).unwrap();
    assert_eq!(moved.changed_files.len(), 2);
    let parsed: toml::Table = fs::read_to_string(&config).unwrap().parse().unwrap();
    let hooks = parsed["hooks"].as_array().unwrap();
    assert_eq!(hooks.len(), 1, "дублей нет");
    assert_eq!(
        hooks[0]["command"].as_str(),
        Some("\"/new/bars-cli\" hook session-start")
    );
    assert_eq!(
        read_json(&home.path().join(".kimi-code/mcp.json"))["mcpServers"]["bars"]["command"],
        "/new/bars-mcp"
    );
}

#[test]
fn kimi_inline_hooks_are_read_updated_and_removed() {
    for initial in [
        "hooks = []\n",
        "hooks = [{ event = 'Notification', command = 'notify' }]\n",
    ] {
        let home = TempDir::new().unwrap();
        let env = env(&home);
        let config = home.path().join(".kimi-code/config.toml");
        write(&config, initial);
        assert!(uninstall(&env, InstallClient::KimiCode).unwrap().is_empty());
        assert_eq!(fs::read_to_string(&config).unwrap(), initial);
        assert!(!config.with_extension("toml.bars-backup").exists());
    }
    for initial in [
        "hooks = []\n",
        "hooks = [{ event = 'Notification', command = 'notify' }, { event = 'SessionStart', command = 'old/bars-cli hook session-start' }]\n",
    ] {
        let home = TempDir::new().unwrap();
        let env = env(&home);
        let config = home.path().join(".kimi-code/config.toml");
        write(&config, initial);
        let foreign = initial.contains("notify");
        assert_eq!(
            super::hook_commands(InstallClient::KimiCode, &config)
                .unwrap()
                .len(),
            usize::from(foreign)
        );
        install(&env, InstallClient::KimiCode).unwrap();
        assert_eq!(
            super::hook_commands(InstallClient::KimiCode, &config).unwrap(),
            [env.hook_command()]
        );
        assert!(
            install(&env, InstallClient::KimiCode)
                .unwrap()
                .changed_files
                .is_empty()
        );
        uninstall(&env, InstallClient::KimiCode).unwrap();
        assert_eq!(
            fs::read_to_string(&config).unwrap().contains("notify"),
            foreign
        );
        // Удаление должно распознавать inline-хук и без предшествующей установки.
        write(&config, initial);
        uninstall(&env, InstallClient::KimiCode).unwrap();
        assert!(
            super::hook_commands(InstallClient::KimiCode, &config)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            fs::read_to_string(&config).unwrap().contains("notify"),
            foreign
        );
    }
}

#[test]
fn kimi_uninstall_removes_only_bars() {
    let home = TempDir::new().unwrap();
    let env = env(&home);
    let mcp = home.path().join(".kimi-code/mcp.json");
    let config = home.path().join(".kimi-code/config.toml");
    write(
        &config,
        "default_model = \"k3\"\n\n[[hooks]]\nevent = \"Notification\"\nmatcher = \"x\"\ncommand = \"notify\"\n",
    );
    install(&env, InstallClient::KimiCode).unwrap();

    let removed = uninstall(&env, InstallClient::KimiCode).unwrap();
    assert_eq!(removed.len(), 5);
    assert_eq!(read_json(&mcp), json!({"mcpServers": {}}));
    let parsed: toml::Table = fs::read_to_string(&config).unwrap().parse().unwrap();
    assert_eq!(parsed["default_model"].as_str(), Some("k3"));
    let hooks = parsed["hooks"].as_array().unwrap();
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0]["command"].as_str(), Some("notify"));
    assert!(!home.path().join(".kimi-code/skills/bars-docs").exists());
    assert!(uninstall(&env, InstallClient::KimiCode).unwrap().is_empty());
}
