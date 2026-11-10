---
name: bars-session-log
description: "Start, maintain or finish a project session journal."
---

# Session journal

Write session records and search queries in the user's language, following explicit project language rules. These agent instructions are in English; session logs and handoffs are user documents, not agent instructions.

Follow the project's own rules and locations first. Otherwise, use `chats/<YYYY-MM-DD>-<topic>/` under its configured documentation root (not necessarily `docs/`). The layout is advice; bars accepts other structures. The topic is 2–4 words in Latin letters joined by hyphens. Each session the user talks to has its own folder; subagents keep no journal: their result goes to the main session's journal. Do not create placeholders or empty folders.

## Choose the right record

| Content | Recommended location | Example and writing rule |
|---|---|---|
| Session journal | Session `handoff.md` (no `log.md`) | Done, Not done, Difficulties (omit when none; holds non-obvious findings and abandoned approaches), Next, headed in the user's language; at most a screen. |
| Enduring research or measurement | `knowledge/<topic>.md` | Method, evidence, findings and limits; a journal entry links to it. A measurement is not a shipped change. |
| Accepted rationale | `adr/NNNN-<topic>.md` | Owner-selected architecture, dated decision and consequences; a proposal is not an accepted ADR. |
| Bug | `backlog/issues.md` | ID, source/date, symptom, known steps, expected/actual, status, verification; do not invent a cause. A handoff line is not the bug's only record. |
| Owner idea | `backlog/ideas.md` | ID, source/date, need, status and design links; not an accepted requirement. |

Update existing documents before creating duplicates. In a code repository, current behaviour and feature requirements belong in `features/<name>/` and shared structure in `architecture/`; in another project, use the existing folders. See `BARS_INSTRUCTION.md` in the documentation root for the full document-kind table, format, link and Index.md rules; `bars-docs` covers searching and reading. Link records instead of copying their text: the result a handoff points to (the research, feature description or ADR written in the session) is a relative Markdown link with a readable name, such as `[delivery research](../../knowledge/delivery.md)`. Other files changed along the way are named without links; the diff already lists them. One link per document in a section; code paths and commits stay in backticks. Update the owning folder's index when adding a topic; individual session folders need no Index.md.

## Start of a session

For a just-saved handoff, read it from disk with `bars_read`; embedding may still be queued. Use `bars_search` with `occurrences=true` for an exact phrase in saved session files. Neither tool reads unsaved editor buffers. Do not force reindex to make a recent journal entry searchable.

Identify the task subject before looking for its history; resolve a stage number through the known project plan/register first. Find the latest `handoff.md` on that topic: session folders are named `<YYYY-MM-DD>-<topic>`, so list the `chats/` folder with your file tools and pick the newest matching topic; search ranks by meaning, not by date. If the path is known, call `bars_read` directly (its `view:"outline"` returns headings); no preliminary search is required.

If the handoff is unknown, search the history folder, for example `bars_search({query:"Последние результаты проверки устройств моделей",in:["chats"]})`. Real folder names come from the session instruction, tool descriptions or the PROJECT part of `bars_status`. Names above come from the suggested layout for code repositories (`features`, `architecture`, `adr`); another project has its own folders. When you look for the current state rather than history, the agent filters: choose the folders with current documents and leave out journals and archives. Several kinds or an unclear location allow the whole project; widen weak/empty scoped results. No filtering quota; do not limit every query to history.

`in` accepts file paths as well as folders: after discovering the two paths, `bars_search({query:"Какие ограничения остались после проверки моделей?",in:["chats/2026-10-18-models/handoff.md","features/models/requirements.md"]})`. Documentation-root and project-root paths both work; disk existence does not imply index membership. For a whole known handoff, prefer read. After an empty answer, verify topic, project and scope; try the whole project if a folder scope was set. Do not infer missing history or reindex automatically.

## handoff.md — at the end of a session

There is no `log.md`: the handoff is the only journal file of a session. Write it at the end, on large milestones of a long session, and at once when you find something non-obvious or abandon an approach (do not wait for the end: a session may break off). Do not write the ordinary course of work, code, command output or what the diff shows; a decision with reasons goes to `adr/`, a durable finding to `knowledge/`, and the handoff keeps one line with a link. When the user ends the session with `/bars-end`, that skill also moves the session's knowledge into the documentation and then writes the handoff. Not longer than a screen. Sections:

- **Done** — what is ready and where it lies: links to the main resulting documents, other changed documents by name, code files and commits in backticks.
- **Not done** — what is left of the task.
- **Difficulties** — what failed, what took the time, what is broken, non-obvious findings, approaches abandoned and why; omit the section when there were none.
- **Next** — where the next session starts.

Section headings are written in the user's language (in Russian: «Сделано», «Не сделано», «Сложности», «Дальше»). When the session continues an earlier one on the same topic, the first line after the title links to that session's handoff: «Предыдущая сессия: [handoff](../<folder>/handoff.md)».

Browser profiles, caches, temporary databases and tool dumps are not committed; raw measurement data needed later goes to `knowledge/<topic>-<date>/` next to the research.

No keys, tokens, passwords or personal data in the journal, even in command output.
