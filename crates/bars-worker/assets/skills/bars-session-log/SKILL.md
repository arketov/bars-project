---
name: bars-session-log
description: "Start, maintain or finish a project session journal."
---

# Session journal

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

Update existing documents before creating duplicates. Current behaviour and feature requirements belong in `features/<name>/`; shared structure belongs in `architecture/`. See `bars-docs` for the full document-kind table, format and Index.md rules. Link records instead of copying their text. Update the owning folder's index when adding a topic; individual session folders need no Index.md.

## Start of a session

Find the latest `handoff.md` on your topic in `docs/chats/` (through bars or by folder names) and read only it. Open `log.md` only when you need the detailed history.

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

- **Done** — what is ready and where it lies (files, commits).
- **Not done** — what is left of the task.
- **Difficulties** — what failed, what took the time, what is broken.
- **Next** — where the next session starts.

No keys, tokens, passwords or personal data in the journal, even in command output.
