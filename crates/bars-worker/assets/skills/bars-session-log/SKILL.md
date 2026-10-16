---
name: bars-session-log
description: How to keep the session journal in docs/chats: log.md for a reason and handoff.md at the end, and how to start from the latest handoff. Use at the start and end of a session and on a non-obvious finding or a change of plan.
---

# Session journal

Every session the user talks to keeps a folder `docs/chats/<YYYY-MM-DD>-<topic>/`. The topic is 2–4 words in Latin letters joined by hyphens. Subagents keep no journal: their result goes to the journal of the main session.

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
