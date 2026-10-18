---
name: bars-docs
description: "Create, edit or move project documentation."
---

# Documentation for bars

bars splits Markdown by headings and searches by section, so the shape of a document affects search quality. The full rules are in `BARS_INSTRUCTION.md` in the documentation root; read it before a larger change. Write documents and search queries in the user's language, following explicit project language rules. English is only for agent instruction documents; it does not set the language of generated documents, queries or UI.

## Choose the subject and sources

Identify the task subject first. Resolve a stage number through the known project plan/register. For a known document use `bars_read` directly; use `bars_outline` for headings. A preliminary search is not required, including for an unindexed Markdown plan read from disk.

For unknown context, search with a concrete question in the user's language. A cross-cutting task may need project-wide search. `modules` selects actual first-level documentation folders, discovered with `bars_projects` when needed, not nested topics. Examples: `bars_rag_search({query:"Какие пожелания есть к обновлению индекса?",modules:["backlog"]})`; `bars_rag_search({query:"Последние результаты проверки устройств моделей",modules:["chats"]})`.

Use `files` for already selected indexed sources: `bars_rag_search({query:"Как выбирается устройство rerank?",files:["features/models/local-models.md","features/settings/settings.md"]})`. Paths are relative to the documentation root; omit `docs/` when that root is `docs`. Read/outline accepts broader disk paths; do not copy them blindly into `files` or infer index membership from disk existence. Examples assume the suggested layout; use known paths in the target project. An early guessed file list may exclude the direct answer.

After zero results, check the subject, project and filters, then reformulate or read a known primary source. Do not infer that a requirement is absent or reindex because of an empty answer. Filtering is optional; choose it for the information needed.

## File format

- `title` in the frontmatter of every file, descriptive; one `#` heading equal to it.
- ATX headings only (`#`…`######`).
- A section stands on its own: no «see above»; the subject is named in the heading.
- Medium-sized sections: huge ones are cut, tiny ones are glued.
- One topic — one file. Do not copy: link to the place that owns the fact.
- State the status at the top of a topic file: what is verified and how, what is not.
- A document describes the current state. When it is outdated, edit or delete it; never append corrections.
- Relative links; moving a file means fixing every link to it.
- No secrets (keys, tokens, passwords).

## Layout (advice)

Project rules and existing locations take priority. Paths below are relative to the configured documentation root, not necessarily `docs/`. Search for the existing document on the topic first. Create a new document only for a new topic; no empty folders or placeholders.

### Requirements, features, research and decisions

| Document kind | Recommended location | Content and example |
|---|---|---|
| Project requirements | Existing main requirements document | Accepted requirements, stable IDs, acceptance criteria; keep the project's name and location. Example: MAIN_IDEA.md in bars. Reference IDs from features, do not duplicate requirements. |
| Feature requirements | `features/<name>/requirements.md` | User behaviour, constraints, acceptance checks, links to project requirements. Example: deleting a project. Distinguish owner decisions from proposals. |
| Feature description | `features/<name>/<topic>.md` | Current behaviour, inputs, results, errors, verification status. Example: `features/search/hybrid-search.md`. |
| Feature plan | `features/<name>/plan.md` | Remaining steps, dependencies, checks; shrink or remove after delivery. Example: instruction delivery steps. |
| Shared architecture | `architecture/<topic>.md` | Shared responsibilities, relationships, data and contracts. Example: `architecture/worker.md`. |
| Accepted decision | `adr/NNNN-<topic>.md` | Status, date, context, decision, consequences, rejected options, Where applied; quote owner decisions with their dates. Example: owner-selected storage. Do not promote agent proposals to accepted decisions. |
| Research or measurement | `knowledge/<topic>.md` | Question, method, sources/evidence, findings, limits, next experiment. Date snapshots, e.g. `search-2026-10-18.md`; measurement is not implementation. |

Feature-only requirements stay in the feature's `requirements.md`, not in the main project-wide requirements document, unless the owner or project rules explicitly request otherwise. Link shared requirement IDs instead of expanding or duplicating the main document.

### Bugs and owner ideas

| Document kind | Recommended location | Content and example |
|---|---|---|
| Bug | `backlog/issues.md` | Stable ID, source/date, symptom, known steps, expected/actual, status, fix and verification links. Example: a failed search. Do not invent a cause or missing steps. |
| Owner idea | `backlog/ideas.md` | Stable ID, source/date, owner's need, status, design and delivery links. Example: a requested button. An idea is not an accepted requirement or decision. |

### Session records and indexes

| Document kind | Recommended location | Content and example |
|---|---|---|
| Session event | `chats/<YYYY-MM-DD>-<topic>/log.md` | Finding, failed approach, plan change or risky step; at most three lines appended without rereading. Example: why an experiment changed. |
| Handoff | `chats/<YYYY-MM-DD>-<topic>/handoff.md` | Done, Not done, Difficulties, Next; at most a screen. Example: where the next session starts. |
| Index | The folder's `Index.md` | Parent link and every file/subfolder with when to read it; update together with contents. Individual session folders are exempt. |

For a mixed task, link the bug register, research, feature description and accepted ADR; do not copy the same fact among them. The journal records session history, not the only copy of a bug or enduring knowledge. Unaccepted proposals remain labelled in research or plans.

- `Index.md` in every folder except session folders: back-link to the parent and a table «file — when to read» listing every file and subfolder. Change it in the same change as the files.
- Module — a first-level subfolder; documents of one area go into one module.
- `knowledge/` — research, measurements, code conventions; write when a finding outlives the session. Snapshot research gets a date in its name.
- `adr/` — one decision per file `NNNN-<topic>.md`: status, date, context, decision (a person's words verbatim, with the date), consequences, rejected alternatives, «Where applied». An ADR is never rewritten: a new decision is a new ADR, the old one changes only its status. Topic documents link to the ADR instead of repeating it.
- `features/<name>/` — a feature as the user sees it: `Index.md` explains it and leads on, `requirements.md` holds requirements, `plan.md` lives while the feature is built, then shrink or delete it.
- `chats/` — session journal (skill `bars-session-log`).

## Before editing

Use bars to check existing sources before editing: read/outline a known document directly, or search unknown context with an appropriate scope. Change the document first, then the code.
