---
name: bars-docs
description: "Use bars tools to search and read project documentation; create, edit or move documentation."
---

# Documentation for bars

bars splits Markdown by headings and searches by section, so the shape of a document affects search quality. The full rules are in `BARS_INSTRUCTION.md` in the documentation root; read it before a larger change. Write documents and search queries in the user's language, following explicit project language rules. English is only for agent instruction documents; it does not set the language of generated documents, queries or UI.

## bars tools: what each one is for

All tools take an optional `project` (default: the project of the session folder) and `format` (`txt` default, `json`). All are read-only except `bars_reindex`, which only refreshes the index.

| Tool | Use it when | Key inputs | Notes |
|---|---|---|---|
| `bars_read` | The document is known (plan, register, contract, a path from a search hit). | `path` | Whole `.md` with line numbers; reads from disk, also files outside the index. |
| `bars_text_search` | Find a literal or regex in saved Markdown, including a recent edit. | `query`, `case_sensitive`, `regex`, `path_filter`, `modules`, `files` | Disk, no embed. First five hits per document; continue one document with its project-relative `path`, `offset`, `revision`. On `stale_revision`, restart. Truncated totals are lower bounds; narrow scope, no global document pagination. |
| `bars_links` | Inspect fresh incoming or outgoing links of one saved document. | project-relative `path`, `direction`, `limit`, `cursor` | Refreshes the link index, not GraphRAG. Follow `next_cursor`; on `stale_cursor`, restart. Check `known`, `has_more`, `limit_reasons`; clipped display fields are not the full source. |
| `bars_outline` | You need the structure of a known document or one part of it before reading. | `path`, `levels`, `under` / `under_line` | Headings with line numbers; cheaper than `bars_read` for long files. |
| `bars_rag_search` | The context is unknown: «where is X described», requirements, decisions, past sessions. | `query`, `modules`, `files`, `limit`, `mode` | Ask a concrete question in the user's language. `mode=mix` adds related sources from the graph, `mode=graph` returns relations; without `mode` the plain hybrid search runs. Each hit has an `id`. |
| `bars_get_similar` | Before adding text: does a description already exist? Before editing: which other sections say the same? | `chunk_id` from a search hit, `modules`, `limit` | Closeness is not duplication; read both sections. An ID goes stale when its document changes (`chunk_not_found`): search again. |
| `bars_get_duplicates` | Auditing a project's docs: repeated descriptions, diverged copies, which source to keep. | `project` (required), `modules`, `limit` | Kinds: exact, near (with changed words, numbers, negations), contained, partial, semantic. Semantic is only a hint; `repeated_block` is probably a template. Read both sections before merging; nothing is changed for you. |
| `bars_projects` | You do not know project names or module names. | none | Registered projects, documentation roots, first-level modules with document counts. |
| `bars_status` | Results look empty or incomplete; check the index before blaming the query. | `project` | Last update, indexing in progress, pending documents, model, `reindex_required`, last error. While indexing, results are incomplete. |
| `bars_reindex` | The status says the index is stale or a document was changed outside the editor and is missing from results. | `project`, `full` | Returns at once; watch `bars_status`. After a model change it fails with `reindex_needs_consent`: tell the user and do not retry. |
| `bars_relations` | What does X depend on, use, implement, verify or supersede? Exact statements with evidence. | `query` or `entity_ref`, `predicates`, `direction`, `polarity` | Ambiguous names return candidates, not a guess. |
| `bars_paths` | How are two things connected? | `from`, `to`, `path_kind`, `max_hops` | A shortest path is a link, not a causal explanation. No path is not proof of no link. |
| `bars_impact` | I am changing X: which documents describe, depend on or verify it? | `roots` (object refs from other graph results) | Result means «re-check these descriptions», not «something is broken». Hyperlinks are not dependencies. |
| `bars_context` | Collect sources for a task grouped by role: requirement, decision, behavior, architecture, constraint, verification. | `query`, `task_profile` (implementation / investigation / decision), `roles`, `token_budget` | Returns primary sources, not an answer; read `missing_roles`. |
| `bars_explore` | Overview of a topic or an audit: topics, isolated documents and broken links, gaps, conflict candidates. | `topic` or `whole_scope=true`, `analysis` (topics / connectivity / coverage / conflict_candidates) | Conflict candidates are candidates, not verdicts. Structural defects and semantic gaps are reported apart. |
| `bars_graph` | Look at a subgraph of documents, sections, chunks or entities. | `view`, `local` + `roots`, `include_paths` | Local mode needs roots; there is no unbounded dump. |

### Choosing a tool

1. Know the document → `bars_read` (or `bars_outline` first for a long one). No search needed, even for an unindexed plan.
2. Do not know where the answer lives → `bars_rag_search` with a subject question. Narrow with `modules` (first-level folders from `bars_projects`: `backlog` for wishes, `chats` for session results) or `files` (already chosen indexed documents, paths from the documentation root). A cross-layer task needs no filter.
3. Need related material around a hit → `bars_get_similar` for same-topic text, `bars_relations` / `bars_paths` for explicit links, `bars_context` for a role-grouped bundle for a task.
4. About to change something → `bars_impact` on the object, then `bars_get_similar` on the section you will edit.
5. Reviewing the documentation as a whole → `bars_explore`, then `bars_graph` to inspect a finding.
6. Zero or odd results → check subject, project and filters, then `bars_status`; reformulate or read the primary source. An empty answer does not prove a requirement is absent, and is not a reason to reindex.

Graph tools read a prebuilt graph; the semantic layer is incomplete, so a missing relation proves nothing. Check `partial`, `coverage` and `evidence` in every answer, and treat similarity as a hint, never as a dependency. Graph results need the graph to be built; on `graph_not_ready` fall back to `bars_rag_search`.

## Freshness after editing

Read/outline/text_search/links read saved disk content, never the window's unsaved editor buffer. Search/similar/duplicates use the embed index: a saved edit can wait for the document pause and queue. `pending` does not identify the reason; `ready` does not prove every file is current. Do not force reindex to bypass normal waiting. Use text_search for an exact recent string and links for fresh document links; GraphRAG has separate generations and may be stale/partial. `bars_graph` is not the window's physical graph. Agents edit files with their own file tools; MCP does not save, move or force-save documents.

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

## Links in the text

A link is a claim that two documents are related. bars builds its document graph from links, and a reader of one search result follows them for details. A link without a relation is noise in both, so link on purpose, not on every mention.

- **Link when the text states a relation** to the target: the statement is based on it (a requirement, decision or research), it replaces, implements, verifies or fixes it, or the details live there. A passing mention is plain words.
- **Once per section.** A document gets one link in a section, at its first relevant mention; later mentions in that section are words. The next section may link again: each section is read on its own.
- **No link bundles.** When one passage relies on more than three documents, put them in one line «Based on: …» at the start of the section instead of scattering links through the paragraph.
- **Do not link** a folder `Index.md` (no «← parent» back-links), lists of files changed in a session, a session in `chats/` from a document of the current state, or files outside the documentation root such as a question register or a plan: name them or use the ID, `OQ-12`.
- Write a relative Markdown link with a readable name: `[worker settings](../settings/settings.md)`, not a path in backticks or «the settings document». Link the document and name the section in words; heading anchors break when a heading changes.
- Check the target exists before linking; find an unknown path with `bars_rag_search`. Never link to a document that is not written yet.
- A new document links to the sources it relies on and is listed in its folder's `Index.md`; a document it replaces or extends links to it.
- Registers and traceability tables are exempt: each row links what it maps.
- Code paths, commands and files outside the documentation stay in backticks.
- Before finishing, reread the changed text: every link is working and states a relation; no document is linked twice in one section.

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
| Session event | `chats/<YYYY-MM-DD>-<topic>/log.md` | Finding, failed approach, plan change or risky step; up to about 400 characters appended without rereading. Example: why an experiment changed. |
| Handoff | `chats/<YYYY-MM-DD>-<topic>/handoff.md` | Done, Not done, Difficulties (omit when none), Next, headed in the user's language; at most a screen. Example: where the next session starts. |
| Index | The folder's `Index.md` | Every file/subfolder by name with when to read it, no links; update together with contents. Individual session folders are exempt. |

For a mixed task, link the bug register, research, feature description and accepted ADR; do not copy the same fact among them. The journal records session history, not the only copy of a bug or enduring knowledge. Unaccepted proposals remain labelled in research or plans.

- `Index.md` in every folder except session folders: a table «file — when to read» listing every file and subfolder by name in backticks, without links and without a back-link to the parent. The folder already relates its files; links from an index would tie every document to it in the graph. Change it in the same change as the files.
- Module — a first-level subfolder; documents of one area go into one module.
- `knowledge/` — research, measurements, code conventions; write when a finding outlives the session. Snapshot research gets a date in its name.
- `adr/` — one decision per file `NNNN-<topic>.md`: status, date, context, decision (a person's words verbatim, with the date), consequences, rejected alternatives, «Where applied». An ADR is never rewritten: a new decision is a new ADR, the old one changes only its status. Topic documents link to the ADR instead of repeating it.
- `features/<name>/` — a feature as the user sees it: `Index.md` explains it and leads on, `requirements.md` holds requirements, `plan.md` lives while the feature is built, then shrink or delete it.
- `chats/` — session journal (skill `bars-session-log`).

## Before editing

Use bars to check existing sources before editing: read/outline a known document directly, or search unknown context with an appropriate scope. Change the document first, then the code.
