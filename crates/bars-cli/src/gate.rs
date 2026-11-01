//! `bars-cli hook pre-tool-use`: гейт «сначала bars» перед чтением документации проекта
//! (`docs/features/clients/clients.md`, «Гейт чтения документации»).
//!
//! Пока в сессии не было ни одного вызова инструмента bars, чтение Markdown из папки
//! документации (Read, Grep, Glob и читающие команды shell) отклоняется с подсказкой. Первый
//! вызов bars снимает блок до конца сессии: отметка лежит во временном файле, ключ — `session_id`.
//! Тексты, которые видит агент, написаны по-английски.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

/// Префикс имён инструментов bars у клиента.
const BARS_TOOLS: &str = "mcp__bars__";

/// Читающие утилиты shell: первое слово звена команды.
const SHELL_READERS: [&str; 7] = ["cat", "head", "tail", "grep", "rg", "sed", "awk"];
const POWERSHELL_READERS: [&str; 3] = ["get-content", "select-string", "type"];

pub const DENY_MESSAGE: &str = "Use bars before reading this project's documentation. \
For a known document call bars_read (text) or bars_outline (headings); for unknown context call \
bars_rag_search with a concrete question in the user's language. Choose actual first-level \
modules from the session instruction or bars_projects: features for contracts, architecture \
for structure, adr for decisions, chats for history in the suggested layout. Several kinds \
or unclear location allow the whole project; widen weak/empty scoped results. No filtering \
quota. files selects known indexed documents (docs-root or project-root paths). The bars tool \
schemas may be deferred: load them with ToolSearch query \
\"select:mcp__bars__bars_rag_search,mcp__bars__bars_outline,mcp__bars__bars_read\". \
After the first bars call reading is allowed for the rest of the session.";

#[derive(Debug, Default, Deserialize)]
pub struct GateInput {
    tool_name: Option<String>,
    tool_input: Option<Value>,
    session_id: Option<String>,
}

/// Решение гейта: `Some(текст)` — отклонить вызов, `None` — пропустить.
pub fn decide(input: &GateInput, docs_root: &str, flag_dir: &Path) -> Option<&'static str> {
    let tool = input.tool_name.as_deref().unwrap_or_default();
    let flag = flag_path(flag_dir, input.session_id.as_deref().unwrap_or("x"));
    if tool.starts_with(BARS_TOOLS) {
        if !flag.exists() {
            let _ = std::fs::write(&flag, b"");
        }
        return None;
    }
    if flag.exists() {
        return None;
    }
    let docs = norm_docs_root(docs_root)?;
    let empty = json!({});
    let args = input.tool_input.as_ref().unwrap_or(&empty);
    reads_docs(tool, args, &docs).then_some(DENY_MESSAGE)
}

/// Ответ хука для клиента: отказ с причиной.
pub fn deny_output(reason: &str) -> Value {
    json!({"hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": "deny",
        "permissionDecisionReason": reason,
    }})
}

fn flag_path(dir: &Path, session: &str) -> PathBuf {
    let safe: String = session
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    dir.join(format!("bars-gate-{safe}.flag"))
}

fn norm(value: &str) -> String {
    value.replace('\\', "/").to_lowercase()
}

/// Корень документации без крайних слэшей; `.` и пустой корень гейт не включают: под него
/// попал бы весь проект.
fn norm_docs_root(docs_root: &str) -> Option<String> {
    let root = norm(docs_root);
    let root = root.trim_matches('/');
    (!root.is_empty() && root != ".").then(|| root.to_string())
}

fn in_chats(path: &str) -> bool {
    path.contains("/chats/") || path.ends_with("/chats")
}

/// Путь лежит в папке документации (но не в журнале сессий).
fn in_docs(path: &str, docs: &str) -> bool {
    let path = format!("/{}", norm(path).trim_start_matches('/'));
    (path.contains(&format!("/{docs}/")) || path.ends_with(&format!("/{docs}"))) && !in_chats(&path)
}

fn text<'a>(args: &'a Value, key: &str) -> &'a str {
    args.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn reads_docs(tool: &str, args: &Value, docs: &str) -> bool {
    match tool {
        "Read" => {
            let path = text(args, "file_path");
            norm(path).ends_with(".md") && in_docs(path, docs)
        }
        "Grep" | "Glob" => {
            let path = text(args, "path");
            if in_chats(&norm(path)) {
                return false;
            }
            if in_docs(path, docs) {
                return true;
            }
            let glob = match text(args, "glob") {
                "" if tool == "Glob" => text(args, "pattern"),
                glob => glob,
            };
            // Шаблон `*.md` вне документации пропускается: путь поиска или явная папка в шаблоне
            // называют другое место; без них поиск идёт от корня проекта и заходит в документацию.
            norm(glob).ends_with(".md") && path.is_empty() && glob_reaches_docs(glob, docs)
        }
        "Bash" | "PowerShell" => shell_reads_docs(text(args, "command"), docs),
        _ => false,
    }
}

/// Шаблон без заданной папки (`*.md`, `**/*.md`) или с папкой документации (`docs/**/*.md`)
/// доходит до документации; шаблон с другой папкой (`crates/**/*.md`) — нет.
fn glob_reaches_docs(glob: &str, docs: &str) -> bool {
    let glob = norm(glob);
    let literal: Vec<&str> = glob
        .split('/')
        .take_while(|part| !part.contains(['*', '?', '[', '{']))
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    literal.is_empty() || in_docs(&format!("{}/", literal.join("/")), docs)
}

/// Звенья конвейера разбираются по отдельности: `| head` в конце команды и путь в тексте
/// коммита не считаются чтением файла.
fn shell_reads_docs(command: &str, docs: &str) -> bool {
    command.split(['|', ';', '&']).any(|segment| {
        let mut words = segment.split_whitespace();
        let Some(first) = words.next().map(str::to_lowercase) else {
            return false;
        };
        let reader =
            SHELL_READERS.contains(&first.as_str()) || POWERSHELL_READERS.contains(&first.as_str());
        reader
            && words.any(|word| {
                let word = word.trim_matches(['\'', '"']);
                norm(word).ends_with(".md") && in_docs(word, docs)
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(tool: &str, args: Value) -> GateInput {
        GateInput {
            tool_name: Some(tool.into()),
            tool_input: Some(args),
            session_id: Some("s1".into()),
        }
    }

    #[test]
    fn denies_docs_reads_until_a_bars_call() {
        let dir = tempfile::tempdir().unwrap();
        let read = input("Read", json!({"file_path": "C:\\p\\docs\\features\\a.md"}));
        assert_eq!(decide(&read, "docs", dir.path()), Some(DENY_MESSAGE));
        assert_eq!(
            decide(
                &input("mcp__bars__bars_read", json!({})),
                "docs",
                dir.path()
            ),
            None
        );
        assert_eq!(decide(&read, "docs", dir.path()), None);
    }

    #[test]
    fn skips_chats_code_and_other_sessions() {
        let dir = tempfile::tempdir().unwrap();
        for (tool, args) in [
            ("Read", json!({"file_path": "/p/docs/chats/x/log.md"})),
            ("Read", json!({"file_path": "/p/src/main.rs"})),
            ("Read", json!({"file_path": "/p/README.md"})),
            ("Grep", json!({"pattern": "x", "path": "/p/docs/chats"})),
            ("Glob", json!({"pattern": "crates/**/*.md"})),
            ("Glob", json!({"pattern": "*.md", "path": "/p/crates"})),
            (
                "Grep",
                json!({"pattern": "x", "path": "/p/src", "glob": "*.md"}),
            ),
            (
                "Grep",
                json!({"pattern": "x", "glob": "crates/*/README.md"}),
            ),
            ("Bash", json!({"command": "git log -- docs/a.md | head"})),
            ("Edit", json!({"file_path": "/p/docs/a.md"})),
        ] {
            assert_eq!(
                decide(&input(tool, args), "docs", dir.path()),
                None,
                "{tool}"
            );
        }
    }

    #[test]
    fn catches_search_and_shell_reads() {
        let dir = tempfile::tempdir().unwrap();
        for (tool, args) in [
            ("Grep", json!({"pattern": "x", "path": "/p/docs"})),
            ("Grep", json!({"pattern": "x", "glob": "*.md"})),
            ("Glob", json!({"pattern": "docs/**/*.md"})),
            ("Glob", json!({"pattern": "**/*.md"})),
            ("Grep", json!({"pattern": "x", "glob": "./docs/*/a.md"})),
            ("Bash", json!({"command": "cat docs/a.md"})),
            (
                "PowerShell",
                json!({"command": "Get-Content 'docs\\a.md' | Select -First 5"}),
            ),
        ] {
            assert_eq!(
                decide(&input(tool, args), "docs", dir.path()),
                Some(DENY_MESSAGE),
                "{tool}"
            );
        }
    }

    #[test]
    fn follows_the_configured_docs_root_and_ignores_a_dot_root() {
        let dir = tempfile::tempdir().unwrap();
        let read = input("Read", json!({"file_path": "/p/documentation/ru/a.md"}));
        assert_eq!(
            decide(&read, "documentation/ru", dir.path()),
            Some(DENY_MESSAGE)
        );
        assert_eq!(decide(&read, "docs", dir.path()), None);
        assert_eq!(decide(&read, ".", dir.path()), None);
    }
}
