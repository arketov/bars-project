//! iterion подключается плагином `<корень>/plugins/bars/`: манифест `plugin.yaml` с MCP-сервером
//! и skill `bars-docs`, без хука и без правки общего `plugins.yaml` (OQ-286, OQ-290).
use std::path::{Path, PathBuf};

use super::{InstallEnv, Result, fsx};

/// Путь skill в манифесте — от папки плагина; iterion называет skill по папке `bars-docs`.
const SKILL_PATH: &str = "skills/bars-docs/SKILL.md";

/// Корень iterion: `ITERION_HOME`, а без неё `~/.iterion`.
pub fn plugin_dir(env: &InstallEnv) -> PathBuf {
    env.iterion_home
        .clone()
        .unwrap_or_else(|| env.home.join(".iterion"))
        .join("plugins")
        .join("bars")
}

pub fn manifest_file(env: &InstallEnv) -> PathBuf {
    plugin_dir(env).join("plugin.yaml")
}

/// Манифест только с полями, известными строгому разбору iterion (`UnmarshalStrict`). Строки
/// записываются как строки JSON: это допустимый YAML и верное экранирование путей Windows.
fn manifest(exe: &Path) -> String {
    let text = |value: &str| serde_json::to_string(value).expect("строка JSON");
    format!(
        "schema_version: 1\n\
         name: bars\n\
         version: {}\n\
         description: {}\n\
         default_enabled: true\n\
         contributes:\n\
         \x20 mcp_servers:\n\
         \x20   - name: bars\n\
         \x20     transport: stdio\n\
         \x20     command: {}\n\
         \x20     args: []\n\
         \x20 skills:\n\
         \x20   - {SKILL_PATH}\n",
        text(env!("CARGO_PKG_VERSION")),
        text("bars: local Markdown knowledge base (MCP server and the bars-docs skill)"),
        text(&exe.display().to_string()),
    )
}

/// Команда MCP из манифеста; `None`, если файла или строки `command:` нет.
pub fn mcp_command(file: &Path) -> Result<Option<String>> {
    let Some(text) = fsx::read_text(file)? else {
        return Ok(None);
    };
    Ok(text.lines().find_map(|line| {
        let value = line.trim().strip_prefix("command:")?;
        serde_json::from_str::<String>(value.trim()).ok()
    }))
}

/// Плагин установлен, если есть его манифест.
pub fn has_mcp(file: &Path) -> Result<bool> {
    Ok(fsx::read_text(file)?.is_some())
}

/// Пишет манифест; уже правильный файл не переписывается.
pub fn install_mcp(file: &Path, exe: &Path) -> Result<bool> {
    let text = manifest(exe);
    if fsx::read_text(file)?.as_deref() == Some(text.as_str()) {
        return Ok(false);
    }
    fsx::write_atomic(file, &text, false)?;
    Ok(true)
}

/// Убирает папку плагина целиком, и только её.
pub fn uninstall(env: &InstallEnv) -> Result<Option<PathBuf>> {
    let dir = plugin_dir(env);
    Ok(fsx::remove_dir(&dir)?.then_some(dir))
}
