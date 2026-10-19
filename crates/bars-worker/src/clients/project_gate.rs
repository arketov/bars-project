//! Гейт «сначала bars» в проектах Claude Code (`docs/features/clients/clients.md`, «Гейт чтения
//! документации»).
//!
//! Запись `PreToolUse` кладётся в `<проект>/.claude/settings.local.json` проектов, которые есть и
//! в реестре bars, и в `~/.claude.json` Claude Code. Личный файл обычно не попадает в git.
//! Без MCP-записи bars в `~/.claude.json` гейт не ставится: звать было бы нечего.

use std::path::{Path, PathBuf};

use super::{InstallEnv, Result, json, lock};

/// Проект реестра bars: корень и папка документации.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateProject {
    pub path: PathBuf,
    pub docs_root: String,
}

/// Файл настроек проекта, в который bars пишет гейт.
pub fn settings_file(project: &Path) -> PathBuf {
    project.join(".claude").join("settings.local.json")
}

/// Ключ сравнения путей: Claude Code пишет их с `/`, на Windows регистр не различается.
fn key(path: &str) -> String {
    let path = path.replace('\\', "/");
    let path = path.trim_end_matches('/');
    if cfg!(windows) {
        path.to_lowercase()
    } else {
        path.to_string()
    }
}

/// Ставит гейт в проекты, известные и bars, и Claude Code. Возвращает изменённые файлы или
/// ошибку проекта; ошибка одного проекта не мешает остальным.
pub fn sync(env: &InstallEnv, projects: &[GateProject]) -> Vec<Result<Option<PathBuf>>> {
    let _guard = lock();
    let claude = env.home.join(".claude.json");
    let known = match json::has_mcp(&claude).and_then(|has| {
        if has {
            json::claude_projects(&claude)
        } else {
            Ok(Vec::new())
        }
    }) {
        Ok(known) => known,
        Err(error) => return vec![Err(error)],
    };
    let known: Vec<String> = known.iter().map(|path| key(path)).collect();
    projects
        .iter()
        .filter(|project| known.contains(&key(&project.path.to_string_lossy())))
        .map(|project| {
            let file = settings_file(&project.path);
            let command = env.gate_command(&project.docs_root);
            json::install_gate(&file, &command).map(|changed| changed.then_some(file))
        })
        .collect()
}

/// Убирает гейт bars из всех проектов Claude Code. Файл, который не разбирается, пропускается.
pub fn remove_all(env: &InstallEnv) -> Vec<PathBuf> {
    let claude = env.home.join(".claude.json");
    let Ok(known) = json::claude_projects(&claude) else {
        return Vec::new();
    };
    known
        .iter()
        .map(|project| settings_file(Path::new(project)))
        .filter(|file| file.is_file())
        .filter(|file| json::uninstall_gate(file).unwrap_or(false))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use bars_protocol::InstallClient;
    use serde_json::{Value, json};
    use tempfile::TempDir;

    use super::*;
    use crate::clients::{install, uninstall};

    fn env(home: &TempDir) -> InstallEnv {
        InstallEnv {
            home: home.path().to_path_buf(),
            mcp_exe: PathBuf::from("/opt/bars/bars-mcp"),
            cli_exe: PathBuf::from("/opt/bars/bars-cli"),
        }
    }

    fn project(dir: &TempDir, name: &str) -> GateProject {
        let path = dir.path().join(name);
        fs::create_dir_all(&path).unwrap();
        GateProject {
            path,
            docs_root: "docs".into(),
        }
    }

    fn settings(project: &GateProject) -> Value {
        serde_json::from_str(&fs::read_to_string(settings_file(&project.path)).unwrap()).unwrap()
    }

    fn claude_knows(home: &TempDir, projects: &[&GateProject]) {
        let known: serde_json::Map<String, Value> = projects
            .iter()
            .map(|p| (p.path.to_string_lossy().replace('\\', "/"), json!({})))
            .collect();
        fs::write(
            home.path().join(".claude.json"),
            json!({"projects": known}).to_string(),
        )
        .unwrap();
    }

    #[test]
    fn installs_only_for_projects_known_to_claude_code_and_bars() {
        let home = TempDir::new().unwrap();
        let work = TempDir::new().unwrap();
        let (both, bars_only) = (project(&work, "both"), project(&work, "bars-only"));
        claude_knows(&home, &[&both]);
        install(&env(&home), InstallClient::ClaudeCode).unwrap();

        let results = sync(&env(&home), &[both.clone(), bars_only.clone()]);
        assert_eq!(results.len(), 1);
        assert!(matches!(&results[0], Ok(Some(file)) if *file == settings_file(&both.path)));
        let group = &settings(&both)["hooks"]["PreToolUse"][0];
        assert_eq!(
            group["matcher"],
            "Read|Grep|Glob|Bash|PowerShell|mcp__bars__.*"
        );
        assert_eq!(
            group["hooks"][0]["command"],
            "\"/opt/bars/bars-cli\" hook pre-tool-use --docs-root \"docs\""
        );
        assert!(!settings_file(&bars_only.path).exists());

        // Повтор ничего не пишет.
        let again = sync(&env(&home), &[both.clone()]);
        assert!(matches!(&again[0], Ok(None)));
    }

    #[test]
    fn needs_the_bars_mcp_entry_and_keeps_foreign_hooks() {
        let home = TempDir::new().unwrap();
        let work = TempDir::new().unwrap();
        let project = project(&work, "p");
        claude_knows(&home, &[&project]);
        assert!(sync(&env(&home), &[project.clone()]).is_empty());
        assert!(!settings_file(&project.path).exists());

        install(&env(&home), InstallClient::ClaudeCode).unwrap();
        fs::create_dir_all(project.path.join(".claude")).unwrap();
        fs::write(
            settings_file(&project.path),
            r#"{"hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "mine"}]}]}}"#,
        )
        .unwrap();
        sync(&env(&home), &[project.clone()]);
        let groups = settings(&project)["hooks"]["PreToolUse"].clone();
        assert_eq!(groups.as_array().unwrap().len(), 2);
        assert_eq!(groups[0]["hooks"][0]["command"], "mine");
    }

    #[test]
    fn uninstall_removes_the_gate_and_keeps_foreign_hooks() {
        let home = TempDir::new().unwrap();
        let work = TempDir::new().unwrap();
        let project = project(&work, "p");
        claude_knows(&home, &[&project]);
        install(&env(&home), InstallClient::ClaudeCode).unwrap();
        fs::create_dir_all(project.path.join(".claude")).unwrap();
        fs::write(
            settings_file(&project.path),
            r#"{"hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "mine"}]}]}}"#,
        )
        .unwrap();
        sync(&env(&home), &[project.clone()]);

        let changed = uninstall(&env(&home), InstallClient::ClaudeCode).unwrap();
        assert!(changed.contains(&settings_file(&project.path)));
        let groups = settings(&project)["hooks"]["PreToolUse"].clone();
        assert_eq!(groups.as_array().unwrap().len(), 1);
        assert_eq!(groups[0]["hooks"][0]["command"], "mine");
    }
}
