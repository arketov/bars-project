//! `bars-cli hook pre-tool-use`: гейт-напоминание «ищи через bars» перед обходом документации
//! проекта (`docs/features/agent-tools/agent-layer-project.md`, «Гейт чтения документации по
//! счётчику»; OQ-295, ISS-109).
//!
//! Гейт не блокирует насовсем: поисковые обходы папки документации (Grep, Glob и `rg`, `grep`,
//! `Select-String`, `findstr` в shell) считаются подряд; каждый шестой и каждая широкая рекурсия
//! получают отказ, а тот же вызов, повторённый сразу, проходит. Любой вызов инструмента bars
//! обнуляет счётчик. Чтение известного файла (Read, `cat`) не считается. Счётчик — файл сессии во
//! временной папке, ключ — `session_id`. Тексты, которые видит агент, написаны по-английски.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

/// Префикс имён инструментов bars у клиента.
const BARS_TOOLS: &str = "mcp__bars__";

/// Каждый такой по счёту обход подряд получает отказ с правом повтора.
const SWEEP_LIMIT: u32 = 6;

/// Поисковые утилиты shell: первое слово звена команды.
const SHELL_SEARCHERS: [&str; 4] = ["grep", "rg", "select-string", "findstr"];

/// Имя файла локального журнала обходов во временной папке.
const LOG_FILE: &str = "bars-gate.log";

const RETRY_HINT: &str = "If you still need this call, repeat it unchanged and it will go through; \
any bars call resets the counter.";

#[derive(Debug, Default, Deserialize)]
pub struct GateInput {
    tool_name: Option<String>,
    tool_input: Option<Value>,
    session_id: Option<String>,
}

/// Состояние гейта одной сессии: число обходов подряд и отпечаток последнего отклонённого вызова.
#[derive(Debug, Default, PartialEq, Eq)]
struct State {
    count: u32,
    denied: String,
}

/// Решение гейта: `Some(текст)` — отклонить вызов, `None` — пропустить.
pub fn decide(input: &GateInput, docs_root: &str, dir: &Path) -> Option<String> {
    let tool = input.tool_name.as_deref().unwrap_or_default();
    let session = input.session_id.as_deref().unwrap_or("x");
    let state_file = state_path(dir, session);
    if tool.starts_with(BARS_TOOLS) {
        save(&state_file, &State::default());
        return None;
    }
    let docs = norm_docs_root(docs_root)?;
    let empty = json!({});
    let args = input.tool_input.as_ref().unwrap_or(&empty);
    if !is_sweep(tool, args, &docs) {
        return None;
    }
    let mut state = load(&state_file);
    let fingerprint = fingerprint(tool, args);
    if !state.denied.is_empty() && state.denied == fingerprint {
        state.denied.clear();
        save(&state_file, &state);
        log(dir, session, state.count, "retry");
        return None;
    }
    state.count += 1;
    let reason = if is_broad(tool, args, &docs) {
        Some(broad_message(state.count))
    } else if state.count.is_multiple_of(SWEEP_LIMIT) {
        Some(counter_message(state.count))
    } else {
        None
    };
    state.denied = if reason.is_some() {
        fingerprint
    } else {
        String::new()
    };
    save(&state_file, &state);
    log(
        dir,
        session,
        state.count,
        if reason.is_some() { "deny" } else { "allow" },
    );
    reason
}

fn counter_message(count: u32) -> String {
    format!(
        "That is documentation sweep number {count} in a row without bars_search. bars_search \
replaces Grep, Glob, rg and Select-String over documentation: one concrete question in the \
user's language returns scored sections with paths; narrow it with in: path prefixes, and use \
occurrences=true only for every occurrence of one literal string. {RETRY_HINT}"
    )
}

fn broad_message(count: u32) -> String {
    format!(
        "This call sweeps the whole documentation folder with a *.md mask (documentation sweep \
number {count} in a row). A broad recursion is slow and noisy: bars_search finds sections by \
meaning, words, names and IDs together, and bars_read opens a known document. {RETRY_HINT}"
    )
}

/// Ответ хука для клиента: отказ с причиной.
pub fn deny_output(reason: &str) -> Value {
    json!({"hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": "deny",
        "permissionDecisionReason": reason,
    }})
}

fn state_path(dir: &Path, session: &str) -> PathBuf {
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
    dir.join(format!("bars-gate-{safe}.count"))
}

/// Файл состояния: первая строка — число обходов, вторая — отпечаток отклонённого вызова.
fn load(path: &Path) -> State {
    let Ok(text) = std::fs::read_to_string(path) else {
        return State::default();
    };
    let mut lines = text.lines();
    State {
        count: lines
            .next()
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or(0),
        denied: lines.next().unwrap_or_default().trim().to_string(),
    }
}

/// Сбой записи не должен ломать сессию: гейт тогда просто пропускает вызовы.
fn save(path: &Path, state: &State) {
    let _ = std::fs::write(path, format!("{}\n{}\n", state.count, state.denied));
}

/// Строка локального журнала: время, сессия, счётчик, исход (`allow`, `deny`, `retry`).
fn log(dir: &Path, session: &str, count: u32, verdict: &str) {
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(LOG_FILE))
    {
        let _ = writeln!(file, "{unix}\t{session}\t{count}\t{verdict}");
    }
}

/// Отпечаток вызова (FNV-1a): отличает повтор того же вызова от другого.
fn fingerprint(tool: &str, args: &Value) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in tool.bytes().chain([0]).chain(args.to_string().bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
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

/// Путь лежит в папке документации (журнал `chats/` считается наравне с остальными папками).
fn in_docs(path: &str, docs: &str) -> bool {
    let path = format!("/{}", norm(path).trim_start_matches('/'));
    path.contains(&format!("/{docs}/")) || path.ends_with(&format!("/{docs}"))
}

fn text<'a>(args: &'a Value, key: &str) -> &'a str {
    args.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// Маска файлов вызова Grep или Glob (у Glob без `glob` — сам шаблон).
fn mask<'a>(tool: &str, args: &'a Value) -> &'a str {
    match text(args, "glob") {
        "" if tool == "Glob" => text(args, "pattern"),
        glob => glob,
    }
}

/// Поисковый обход документации: Grep и Glob по её папке или по маске `*.md` от корня проекта,
/// `rg`, `grep`, `Select-String`, `findstr` с путём документации. Чтение известного файла не
/// считается.
fn is_sweep(tool: &str, args: &Value, docs: &str) -> bool {
    match tool {
        "Grep" | "Glob" => {
            let path = text(args, "path");
            if norm(path).ends_with(".md") {
                return false;
            }
            if in_docs(path, docs) {
                return true;
            }
            // Шаблон `*.md` вне документации пропускается: путь поиска или явная папка в шаблоне
            // называют другое место; без них поиск идёт от корня проекта и заходит в документацию.
            let glob = mask(tool, args);
            norm(glob).ends_with(".md") && path.is_empty() && glob_reaches_docs(glob, docs)
        }
        "Bash" | "PowerShell" => shell_sweeps_docs(text(args, "command"), docs),
        _ => false,
    }
}

/// Литеральная папка в начале шаблона: `docs/**/*.md` → `docs`.
fn glob_literal(glob: &str) -> String {
    let glob = norm(glob);
    glob.split('/')
        .take_while(|part| !part.contains(['*', '?', '[', '{']))
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>()
        .join("/")
}

/// Шаблон без заданной папки (`*.md`, `**/*.md`) или с папкой документации (`docs/**/*.md`)
/// доходит до документации; шаблон с другой папкой (`crates/**/*.md`) — нет.
fn glob_reaches_docs(glob: &str, docs: &str) -> bool {
    let literal = glob_literal(glob);
    literal.is_empty() || in_docs(&format!("{literal}/"), docs)
}

/// Широкая рекурсия: маска `*.md` по всей папке документации или от корня проекта (без пути и
/// без папки в шаблоне). Подпапка документации (`docs/features`) не широкая.
fn is_broad(tool: &str, args: &Value, docs: &str) -> bool {
    if !matches!(tool, "Grep" | "Glob") {
        return false;
    }
    let glob = mask(tool, args);
    if !norm(glob).ends_with(".md") {
        return false;
    }
    let path = norm(text(args, "path"));
    let literal = glob_literal(glob);
    let base = [path.trim_matches('/'), literal.as_str()]
        .iter()
        .filter(|part| !part.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join("/");
    base.is_empty() || (in_docs(&base, docs) && format!("/{base}").ends_with(&format!("/{docs}")))
}

/// Звенья конвейера разбираются по отдельности: `| head` в конце команды и путь в тексте
/// коммита не считаются обходом.
fn shell_sweeps_docs(command: &str, docs: &str) -> bool {
    command.split(['|', ';', '&']).any(|segment| {
        let mut words = segment.split_whitespace();
        let Some(first) = words.next().map(str::to_lowercase) else {
            return false;
        };
        SHELL_SEARCHERS.contains(&first.as_str())
            && words.any(|word| in_docs(word.trim_matches(['\'', '"']), docs))
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

    fn grep(n: usize) -> GateInput {
        input(
            "Grep",
            json!({"pattern": format!("x{n}"), "path": "/p/docs/features"}),
        )
    }

    #[test]
    fn denies_every_sixth_sweep_and_lets_a_repeat_through() {
        let dir = tempfile::tempdir().unwrap();
        for n in 1..=5 {
            assert_eq!(decide(&grep(n), "docs", dir.path()), None, "sweep {n}");
        }
        let sixth = grep(6);
        let reason = decide(&sixth, "docs", dir.path()).expect("sixth is denied");
        assert!(reason.contains("number 6"), "{reason}");
        assert_eq!(decide(&sixth, "docs", dir.path()), None, "repeat passes");
        // Повтор не считается: седьмой обход — первый после отказа, следующий отказ на двенадцатом.
        for n in 7..=11 {
            assert_eq!(decide(&grep(n), "docs", dir.path()), None, "sweep {n}");
        }
        assert!(decide(&grep(12), "docs", dir.path()).is_some());
    }

    #[test]
    fn a_bars_call_resets_the_counter() {
        let dir = tempfile::tempdir().unwrap();
        for n in 1..=5 {
            assert_eq!(decide(&grep(n), "docs", dir.path()), None);
        }
        let bars = input("mcp__bars__bars_search", json!({}));
        assert_eq!(decide(&bars, "docs", dir.path()), None);
        for n in 6..=10 {
            assert_eq!(decide(&grep(n), "docs", dir.path()), None, "sweep {n}");
        }
        assert!(decide(&grep(11), "docs", dir.path()).is_some());
    }

    #[test]
    fn a_different_call_after_a_denial_is_not_a_repeat() {
        let dir = tempfile::tempdir().unwrap();
        for n in 1..=5 {
            decide(&grep(n), "docs", dir.path());
        }
        assert!(decide(&grep(6), "docs", dir.path()).is_some());
        assert_eq!(decide(&grep(7), "docs", dir.path()), None);
        // Отказ уже не ждёт повтора: шестой вызов теперь обычный обход.
        assert!(decide(&grep(6), "docs", dir.path()).is_none());
    }

    #[test]
    fn broad_recursion_is_denied_once_and_repeat_passes() {
        let dir = tempfile::tempdir().unwrap();
        for (tool, args) in [
            ("Glob", json!({"pattern": "**/*.md"})),
            ("Glob", json!({"pattern": "docs/**/*.md"})),
            ("Grep", json!({"pattern": "x", "glob": "*.md"})),
            (
                "Grep",
                json!({"pattern": "x", "path": "/p/docs", "glob": "*.md"}),
            ),
        ] {
            let call = input(tool, args);
            let reason = decide(&call, "docs", dir.path()).expect("broad is denied");
            assert!(reason.contains("whole documentation folder"), "{tool}");
            assert_eq!(decide(&call, "docs", dir.path()), None, "{tool} repeat");
        }
    }

    #[test]
    fn narrow_searches_and_known_reads_are_not_counted() {
        let dir = tempfile::tempdir().unwrap();
        for (tool, args) in [
            ("Read", json!({"file_path": "/p/docs/features/a.md"})),
            ("Read", json!({"file_path": "/p/docs/chats/x/handoff.md"})),
            ("Read", json!({"file_path": "/p/src/main.rs"})),
            ("Grep", json!({"pattern": "x", "path": "/p/docs/a.md"})),
            ("Grep", json!({"pattern": "x", "path": "/p/src"})),
            ("Glob", json!({"pattern": "crates/**/*.md"})),
            ("Glob", json!({"pattern": "*.md", "path": "/p/crates"})),
            (
                "Grep",
                json!({"pattern": "x", "path": "/p/src", "glob": "*.md"}),
            ),
            ("Bash", json!({"command": "cat docs/a.md"})),
            ("Bash", json!({"command": "git log -- docs/a.md | head"})),
            ("Edit", json!({"file_path": "/p/docs/a.md"})),
        ] {
            for _ in 0..8 {
                assert_eq!(
                    decide(&input(tool, args.clone()), "docs", dir.path()),
                    None,
                    "{tool}"
                );
            }
        }
    }

    #[test]
    fn counts_folder_sweeps_in_shell_and_in_chats() {
        let dir = tempfile::tempdir().unwrap();
        let calls = [
            input("Grep", json!({"pattern": "a", "path": "/p/docs/chats"})),
            input("Bash", json!({"command": "rg foo docs"})),
            input("Bash", json!({"command": "grep -r foo docs/features"})),
            input(
                "PowerShell",
                json!({"command": "Select-String -Path 'docs\\*' -Pattern x"}),
            ),
            input("PowerShell", json!({"command": "findstr /s x docs\\a.md"})),
            input("Grep", json!({"pattern": "b", "path": "/p/docs/adr"})),
        ];
        for call in &calls[..5] {
            assert_eq!(decide(call, "docs", dir.path()), None);
        }
        assert!(decide(&calls[5], "docs", dir.path()).is_some());
    }

    #[test]
    fn follows_the_configured_docs_root_and_ignores_a_dot_root() {
        let dir = tempfile::tempdir().unwrap();
        let broad = input("Glob", json!({"pattern": "documentation/ru/**/*.md"}));
        assert!(decide(&broad, "documentation/ru", dir.path()).is_some());
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(decide(&broad, "docs", dir.path()), None);
        assert_eq!(decide(&broad, ".", dir.path()), None);
    }
}
