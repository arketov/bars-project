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
| Session event | Session `log.md` | Failed experiment or changed approach; at most three lines, append without rereading. |
| Session handoff | Session `handoff.md` | Done, Not done, Difficulties, Next; at most a screen. |
| Enduring research or measurement | `knowledge/<topic>.md` | Method, evidence, findings and limits; a journal entry links to it. A measurement is not a shipped change. |
| Accepted rationale | `adr/NNNN-<topic>.md` | Owner-selected architecture, dated decision and consequences; a proposal is not an accepted ADR. |
| Bug | `backlog/issues.md` | ID, source/date, symptom, known steps, expected/actual, status, verification; do not invent a cause. A log entry is not the bug's only record. |
| Owner idea | `backlog/ideas.md` | ID, source/date, need, status and design links; not an accepted requirement. |

Update existing documents before creating duplicates. Current behaviour and feature requirements belong in `features/<name>/`; shared structure belongs in `architecture/`. See `bars-docs` for the full document-kind table, format, link and Index.md rules. Link records instead of copying their text: the result a log entry or handoff points to (the research, feature description or ADR written in the session) is a relative Markdown link with a readable name, such as `[delivery research](../../knowledge/delivery.md)`. Other files changed along the way are named without links; the diff already lists them. One link per document in a section; code paths and commits stay in backticks. Update the owning folder's index when adding a topic; individual session folders need no Index.md.

## Start of a session

For a just-saved handoff, read it from disk with `bars_read`; embedding may still be queued. Use `bars_text_search` for an exact phrase in saved session files. Neither tool reads unsaved editor buffers. Do not force reindex to make a recent journal entry searchable.

Identify the task subject before looking for its history; resolve a stage number through the known project plan/register first. Find the latest `handoff.md` on that topic in `docs/chats/`. If the path is known, call `bars_read` directly, or `bars_outline` for headings; no preliminary search is required. Open `log.md` only when you need detailed history.

If the handoff is unknown, search session history, for example `bars_rag_search({query:"Последние результаты проверки устройств моделей",modules:["chats"]})`. Use the project's actual first-level module name; discover it with `bars_projects` when needed. For cross-cutting current contracts, use a broader scope instead of restricting every query to history.

`files` is for already selected indexed sources, not guessed names. Example after discovering the two paths: `bars_rag_search({query:"Какие ограничения остались после проверки моделей?",files:["chats/2026-10-18-models/handoff.md","features/models/requirements.md"]})`. These illustrative paths are relative to the documentation root, without `docs/` when that root is `docs`; disk existence does not imply index membership. For a whole known handoff, prefer read. After an empty answer, verify topic, project and scope; do not infer that no prior work exists or reindex automatically.

## log.md — only for a reason

Write when:

- you found something non-obvious that affects further work;
- an approach did not work: what was tried and why it was abandoned;
- the plan changes;
- you are about to take a risky step: deletion, migration, rewriting many files.

Do not write the ordinary course of work, code, command output or what the diff shows.

Saving tokens:

- an entry is at most three lines;
- append to the end of the file without reading it, for example with `>>`;
- a decision with reasons goes to `adr/`, a durable finding to `knowledge/`; the journal keeps one line with a link.

## handoff.md — at the end of a session

Write it at the end, and on large milestones of a long session. Not longer than a screen. Sections:

- **Done** — what is ready and where it lies: links to the main resulting documents, other changed documents by name, code files and commits in backticks.
- **Not done** — what is left of the task.
- **Difficulties** — what failed, what took the time, what is broken.
- **Next** — where the next session starts.

No keys, tokens, passwords or personal data in the journal, even in command output.
