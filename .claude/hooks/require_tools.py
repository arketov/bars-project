#!/usr/bin/env python3
"""PreToolUse-хук bars-project: подсказывает начинать код с semble / serena.

Пока в сессии не было ни одного вызова semble/serena, первое чтение *.rs
(Read/Grep/Glob и читающая команда Bash) отклоняется с подсказкой. Подсказка
показывается один раз за сессию: если инструментов в сессии нет, повторное
чтение проходит. Первый вызов semble/serena снимает отказ совсем. Документацию
(docs/**/*.md) этот хук не проверяет: это делает гейт bars из
`.claude/settings.local.json`. Отметки лежат во временном файле, ключ — session_id.
"""
import json
import os
import re
import sys
import tempfile

CODE_TOOLS = ("mcp__semble__", "mcp__serena__")
READ_TOOLS = {"Read", "Grep", "Glob"}
SHELL_TOOLS = {"Bash", "PowerShell"}
# читающая утилита как первое слово звена команды
SHELL_READ = re.compile(
    r"^\s*(cat|head|tail|grep|rg|sed|awk|Get-Content|Select-String)\s"
)

CODE_MSG = (
    "Explore the project code through semble and serena first: mcp__semble__search "
    "(where it is implemented, query in English), then serena get_symbols_overview / "
    "find_symbol. The schemas may be deferred: load them with ToolSearch "
    "\"select:mcp__semble__search,mcp__serena__get_symbols_overview,"
    "mcp__serena__find_symbol,mcp__serena__activate_project\". Serena activates itself "
    "in this project (--project-from-cwd); read its manual with "
    "mcp__serena__initial_instructions. If these tools are not available in this "
    "session, repeat the call: this notice is shown once, and after the first semble "
    "or serena call it is not shown at all."
)


def norm(value):
    return str(value or "").replace("\\", "/").lower()


def is_code(path):
    return path.endswith(".rs")


def reads_code(tool, inp):
    if tool == "Read":
        return is_code(norm(inp.get("file_path")))
    if tool in ("Grep", "Glob"):
        path = norm(inp.get("path"))
        glob = norm(inp.get("glob") or (inp.get("pattern") if tool == "Glob" else ""))
        return inp.get("type") == "rust" or glob.endswith(".rs") or "/crates" in "/" + path
    if tool in SHELL_TOOLS:
        # звенья конвейера разбираем по отдельности: `| head` в конце команды
        # или путь в тексте коммита — это не чтение файла
        for segment in re.split(r"[|;&]+", inp.get("command") or ""):
            if SHELL_READ.search(segment) and re.search(r"\.rs\b", norm(segment)):
                return True
    return False


def main():
    try:
        data = json.loads(sys.stdin.buffer.read().decode("utf-8"))
    except Exception:
        return 0
    tool = data.get("tool_name", "")
    state_path = os.path.join(
        tempfile.gettempdir(), "bars-tools-%s.json" % re.sub(r"\W", "_", data.get("session_id", "x"))
    )
    try:
        with open(state_path, encoding="utf-8") as f:
            state = json.load(f)
    except Exception:
        state = {}

    def save(key):
        state[key] = True
        with open(state_path, "w", encoding="utf-8") as f:
            json.dump(state, f)

    if tool.startswith(CODE_TOOLS):
        if not state.get("code"):
            save("code")
        return 0

    if tool not in READ_TOOLS | SHELL_TOOLS:
        return 0
    if reads_code(tool, data.get("tool_input") or {}) and not state.get("code") and not state.get("told"):
        save("told")
        print(json.dumps({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": CODE_MSG,
            }
        }))  # ensure_ascii: вывод не зависит от кодировки консоли Windows
    return 0


if __name__ == "__main__":
    sys.exit(main())
