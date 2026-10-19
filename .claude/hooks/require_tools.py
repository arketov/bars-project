#!/usr/bin/env python3
"""PreToolUse-хук bars-project: заставляет начинать с bars / semble / serena.

Пока в сессии не было ни одного вызова bars (для docs/**/*.md) или
semble/serena (для *.rs), Read/Grep/Glob и чтение через Bash по таким файлам
отклоняются с подсказкой, какой инструмент вызвать. Первый вызов нужного
инструмента снимает блок до конца сессии. Отметки лежат во временном файле,
ключ — session_id.
"""
import json
import os
import re
import sys
import tempfile

DOCS_TOOLS = "mcp__bars__"
CODE_TOOLS = ("mcp__semble__", "mcp__serena__")
READ_TOOLS = {"Read", "Grep", "Glob"}
SHELL_TOOLS = {"Bash", "PowerShell"}
# читающая утилита как первое слово звена команды
SHELL_READ = re.compile(
    r"^\s*(cat|head|tail|grep|rg|sed|awk|Get-Content|Select-String)\s"
)

DOCS_MSG = (
    "Identify the task subject first, using the known plan/register. For a known "
    "document call bars_read or bars_outline directly; for unknown context call "
    "bars_rag_search with a concrete question and a suitable scope. modules:[\"backlog\"] "
    "- wishes; files - chosen indexed paths relative to the docs root, without the root "
    "prefix. The bars tool schemas are deferred: load them with ToolSearch query "
    "\"select:mcp__bars__bars_rag_search,mcp__bars__bars_outline,"
    "mcp__bars__bars_read\". After the first bars call Read is allowed."
)
CODE_MSG = (
    "Explore the project code through semble and serena first: mcp__semble__search "
    "(where it is implemented, query in English), then serena get_symbols_overview / "
    "find_symbol. The schemas are deferred: load them with ToolSearch "
    "\"select:mcp__semble__search,mcp__serena__get_symbols_overview,"
    "mcp__serena__find_symbol,mcp__serena__activate_project\". Serena activates itself "
    "in this project (--project-from-cwd); read its manual with "
    "mcp__serena__initial_instructions. After the first semble or serena call Read "
    "is allowed."
)


def norm(value):
    return str(value or "").replace("\\", "/").lower()


def is_docs(path):
    path = "/" + path.lstrip("/")
    return "/docs/" in path and "/docs/chats/" not in path and path.endswith(".md")


def is_code(path):
    return path.endswith(".rs")


def classify(tool, inp):
    """Возвращает 'docs', 'code' или None."""
    if tool == "Read":
        path = norm(inp.get("file_path"))
        return "docs" if is_docs(path) else "code" if is_code(path) else None
    if tool in ("Grep", "Glob"):
        path = norm(inp.get("path"))
        glob = norm(inp.get("glob") or (inp.get("pattern") if tool == "Glob" else ""))
        if inp.get("type") == "rust" or glob.endswith(".rs") or "/crates" in "/" + path:
            return "code"
        if glob.endswith(".md") or is_docs(path + "/x.md") and "/docs" in path:
            return "docs"
        return None
    if tool in SHELL_TOOLS:
        # звенья конвейера разбираем по отдельности: `| head` в конце команды
        # или путь в тексте коммита — это не чтение файла
        for segment in re.split(r"[|;&]+", inp.get("command") or ""):
            if not SHELL_READ.search(segment):
                continue
            seg = norm(segment)
            if re.search(r"\.rs\b", seg):
                return "code"
            if re.search(r"docs/(?!chats/)[^\s'\"]*\.md\b", seg):
                return "docs"
    return None


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

    if tool.startswith(DOCS_TOOLS) or tool.startswith(CODE_TOOLS):
        key = "docs" if tool.startswith(DOCS_TOOLS) else "code"
        if not state.get(key):
            state[key] = True
            with open(state_path, "w", encoding="utf-8") as f:
                json.dump(state, f)
        return 0

    if tool not in READ_TOOLS | SHELL_TOOLS:
        return 0
    kind = classify(tool, data.get("tool_input") or {})
    if kind and not state.get(kind):
        print(json.dumps({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": DOCS_MSG if kind == "docs" else CODE_MSG,
            }
        }))  # ensure_ascii: вывод не зависит от кодировки консоли Windows
    return 0


if __name__ == "__main__":
    sys.exit(main())
