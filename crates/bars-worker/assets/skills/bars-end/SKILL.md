---
name: bars-end
description: "Explicit-invocation only: the user calls /bars-end (Codex: $bars-end) at the end of a session to bring the project documentation up to date with what the session did and learned. Do not run it on your own."
disable-model-invocation: true
---

# End of session: bring the documentation up to date

The user ends the session with this skill. Your job is to move everything the session did, decided and learned into the project documentation, so that the next session and the people on the project find it in its owning document, not only in this conversation. Write documents in the user's language, following explicit project language rules; this instruction is in English only because it is for agents.

Project rules come first: its instruction files (`AGENTS.md`, `CLAUDE.md` and similar), its documentation layout, its question register and plan conventions. The layout below is advice. `BARS_INSTRUCTION.md` in the documentation root owns document kinds, format, links and `Index.md`; skill `bars-docs` covers searching and reading; use `bars-session-log` for the journal.

Do not change code. Do not commit unless the user asked for a commit in this request or the project rules require it.

## 1. Collect what happened

Sources, all of them:

- this conversation, from the first message;
- `git status`, `git diff` (staged and unstaged) and `git log` of the commits made during the session;
- the session `handoff.md`, if one was already written on a milestone.

Write a private inventory, one line per fact:

- changed behaviour, contracts, settings, commands, defaults;
- decisions of the user, verbatim with the date;
- findings, measurements, root causes;
- approaches that failed and what finally worked;
- bugs found and ideas the user voiced;
- open questions and what was not verified;
- knowledge about the project and its tooling: build, hooks, environment traps, the way a check must be run.

A fact you are unsure of stays a question or «not verified»; never record a guess as a fact.

## 2. Route every fact to its owner

| Fact | Owner (recommended location) |
|---|---|
| Current behaviour, feature contract | `features/<name>/` |
| Shared structure, data, contracts between parts | `architecture/` |
| Research, measurement, project or tooling knowledge | `knowledge/<topic>.md` |
| User-visible change in how to do something | `manual/` |
| Bug | `backlog/issues.md` |
| User idea | `backlog/ideas.md` |
| Open question | the project's question register |
| Decision of the user with rationale | `adr/NNNN-<topic>.md` — propose only |
| Requirement change | the main requirements document — propose only |
| Plan change | the project plan — propose only |
| Rule for agents | `AGENTS.md`, `CLAUDE.md` — propose only |
| What happened in the session | `chats/<YYYY-MM-DD>-<topic>/handoff.md` |

Rows marked «propose only» are not edited without the user's yes in this conversation. Leave the agent client's private memory alone: project knowledge belongs in the documentation.

## 3. Find the existing document, then edit it

For each fact:

1. Find the document that owns the topic: `bars_search` with a subject question or the exact name, number or value in quotes, `bars_read` for a known path (its `view:"outline"` returns headings).
2. Before adding text, check that the description does not already exist elsewhere (`bars_search` with `like:"path:lines"` on a hit). Edit the owner; create a new file only for a new topic, and list it in its folder's `Index.md`.
3. The document describes the current state: replace outdated statements, do not append corrections. Update the status line at the top: what is verified and how, what is not.
4. Search the whole documentation for old names, values and claims the session made obsolete (`bars_search` with the old name or value in quotes, then `bars_search` with `occurrences=true` for the complete list of its occurrences). Fix or delete every stale repeat, or link to the owner instead of repeating it.

Keep the scope to what the session touched and its stale repeats. A full audit of the project documentation is a separate task; suggest it if you saw many problems.

## 4. Check the result

- Every changed file has frontmatter `title` and self-contained sections.
- Every new or renamed file is in its folder's `Index.md`.
- Links are relative, point to existing files, and state a relation; check outgoing links of changed documents with `bars_read` (`view:"links"`).
- No keys, tokens, passwords or personal data.
- Reread the diff of the documentation once.

## 5. Write the handoff

Write or update `handoff.md` by `bars-session-log`: Done, Not done, Difficulties, Next, headed in the user's language (in Russian: «Сделано», «Не сделано», «Сложности», «Дальше»), with a link to the previous handoff when the session continues a topic. In Difficulties, list the nuances of the session and how each was solved, with a link to the document where the lasting knowledge now lives. At most a screen.

## 6. Report to the user

In the user's language:

- a table «fact → where it is recorded»;
- proposed edits that wait for a yes: requirements, ADR, plan, agent rules — each with the exact text;
- what remains unverified or open, with question numbers;
- the list of changed files and a proposed commit message; commit only when asked.
