# BARS_INSTRUCTION.md

This file is placed in the documentation root by bars, a local search tool over Markdown. It is addressed to the models (and people) who write documents. bars overwrites it with every new version, so edits made here are not kept. Project-specific rules belong in the project's own `AGENTS.md` / `CLAUDE.md`.

Write documents in the language the project uses. This file only fixes the structure.

## How bars reads documents

bars splits each Markdown file into sections by headings and indexes every section on its own. A search result is one section: its text, its heading chain, the file path and the file `title`. The reader of a result does not see the neighbouring sections. Every rule below follows from this.

## File format

- **`title` in the frontmatter** of every document. It is shown in search results. Make it descriptive: name the topic and what the file covers, not just «Notes» or «Overview».
- **One `#` heading per file, equal to the `title`.**
- **ATX headings only**: `#` … `######`. Underlined headings (`===`, `---`) are invisible to bars.
- **No secrets** in documentation: keys, tokens, passwords, personal data.
- **Relative links** between documents. Moving or renaming a file means fixing every link to it.
- Only `.md` files are indexed. Mockups, JSON evidence, scripts and images may lie next to documents, but list them in the folder's `Index.md`.

## Sections

- **A section stands on its own.** Never write «see above», «as described earlier», «the previous section». Name the subject in the heading and in the first sentence.
- **Headings say what the section is about.** The heading chain is part of the text that is embedded: «Port setting of the worker» is found, «Settings» is found worse.
- **Medium-sized sections.** A very long section is cut into several chunks that share one heading chain; a tiny one is glued to its neighbour. Prefer several medium sections to one huge one.
- **One topic — one file.** When a file grows into several topics, split it by topic and link the parts from the folder's `Index.md`. Do not collect everything into one big file.
- **State the status at the top of a topic file**, in one line: what is verified and how (tests, a live run, a static reading of the code), what is not verified, what is open. A reader must not mistake a plan for a fact.
- **Do not copy, link.** A requirement, a decision or a definition lives in exactly one place; other documents refer to it by link or by its number. Copies drift apart and then bars finds both versions.

## Current state, not history

- A document describes **how things are now**. When something becomes outdated, edit or delete it. Do not append corrections or «update» / «continuation» layers to the end: bars searches everything it finds, and an old paragraph is returned as readily as the new one.
- History lives only in two places: `adr/` (why a decision was made) and `chats/` (what a session did). Nothing else keeps a story.
- When two documents disagree, the one that describes the current product wins over research and session notes. Say so in the root `Index.md` if the project has such a rule.
- Write a document when there is something to document: a made decision, a finished piece of work, a finding that outlives the session. Do not create empty folders, stubs or «to be filled later» templates.
- **Change the document first, then the code**, when the project has requirements written down.

## Layout of the documentation

This is advice, not a rule: bars indexes any structure. Read the project's own rules and existing layout first; they take priority. Update an existing document on the topic before creating another. Do not create empty folders or placeholder documents. Paths below are relative to the configured documentation root, which need not be named `docs`.

```text
docs/
  Index.md                      table of contents of the whole documentation
  <module>/                     one folder per area; own Index.md
    Index.md
    <topic>.md
  knowledge/                    development knowledge: research, measurements, code conventions
  architecture/                 structure shared by several features
  adr/                          decisions, one file NNNN-<topic>.md
  backlog/                      bugs and owner ideas
    issues.md
    ideas.md
  features/<name>/              a feature as the user sees it
    Index.md
    requirements.md
    plan.md
  chats/<YYYY-MM-DD>-<topic>/   journal of one session
    log.md
    handoff.md
```

### Choose a document: requirements, features, research and decisions

| Document kind | Recommended location | What to write and example |
|---|---|---|
| Project requirements | The project's existing main requirements document | Accepted requirements, stable IDs and acceptance criteria. Keep its existing name; feature documents reference IDs instead of copying requirements. Example: MAIN_IDEA.md in bars. |
| Feature requirements | `features/<name>/requirements.md` | User-visible behaviour, constraints, acceptance checks and links to project requirements; separate owner decisions from proposals. Example: expected behaviour when deleting a project. |
| Feature description | `features/<name>/<topic>.md` | Current behaviour, inputs, outputs, errors and verification status. Example: `features/search/hybrid-search.md`. |
| Feature plan | `features/<name>/plan.md` | Remaining steps, dependencies and checks; shrink or remove when finished. Example: instruction delivery steps. |
| Shared architecture | `architecture/<topic>.md` | Responsibilities, relationships, data and contracts shared by features. Example: `architecture/worker.md`. |
| Accepted decision and rationale | `adr/NNNN-<topic>.md` | Status, date, context, decision, consequences, rejected alternatives, Where applied; quote the owner's decision with its date. Example: the owner selected a storage design. An agent proposal is not an accepted decision. |
| Research, comparison or measurement | `knowledge/<topic>.md` | Question, method, sources/evidence, findings, limits, next discriminating experiment. Date snapshot filenames, e.g. `search-2026-10-18.md`. A measurement does not establish implementation. |

### Where to record bugs and owner ideas

Use the project's existing registers when provided; otherwise `backlog/` is the recommended location. Preserve existing IDs and authorship.

| Document kind | Recommended location | What to write and example |
|---|---|---|
| Bug | `backlog/issues.md` | Stable ID, source/date, symptom, known reproduction steps, expected/actual behaviour, status, fix and verification links. Example: search fails for a known input. Do not invent a cause or missing steps. |
| Owner idea | `backlog/ideas.md` | Stable ID, source/date, owner's need, status, links to design and delivery. Example: a new search button. An idea is not yet an accepted requirement or decision. |

### Where to record session events, handoffs and indexes

| Document kind | Recommended location | What to write and example |
|---|---|---|
| Session event | `chats/<YYYY-MM-DD>-<topic>/log.md` | At most three lines for a finding, failed approach, changed plan or risky step; append without rereading. Example: why an experiment changed. |
| Session handoff | `chats/<YYYY-MM-DD>-<topic>/handoff.md` | Done, Not done, Difficulties, Next; at most a screen. Example: the check the next session should run. |
| Folder index | That folder's `Index.md` | Parent link and each file/subfolder with when to read it; update together with contents. Individual session folders are exempt. |

A mixed task can produce linked documents: bug in the register, measured cause in knowledge, accepted behaviour in the feature, architectural rationale in an ADR. Keep one source for each fact. Session history stays in the journal; current product behaviour stays in the feature. Unaccepted proposals remain labelled proposals in research or plans.

### Modules

- A **module is a first-level subfolder** of the documentation root; everything deeper belongs to the module. Put documents of one area into one module and give it a new folder when its first document appears.
- Common structure that many features rely on (stack, processes, data schema) is kept in its own module next to the features, for example `architecture/`.
- Large knowledge bases are split into modules by area (for example `app/`, `runtime/`, `native/`, `research/`), each with its own `Index.md`, and a hub `Index.md` above them that holds a table «topic → current state → file».

### Index.md in every folder

- **Every folder has an `Index.md`**, except the session folders in `chats/`. The letter case of the name does not matter, but never keep two files that differ only by case in one folder: on Windows and macOS they are one file.
- The index lists **every file and every subfolder** as a link with a one-line description. The usual form is a table with two columns: «File» and «When to read».
- Start the index with a back-link to the parent: `[← Parent index](../Index.md)`.
- Adding, renaming or deleting a file changes the `Index.md` of its folder **in the same change**.
- The root `Index.md` answers «where do I look for X»: a table «question → document», then the list of all modules.

### knowledge/ — development knowledge

Research, comparisons of libraries, measurements, code conventions, found pitfalls. Write when a finding is useful beyond one session.

- Name research files with a date when the result is a snapshot: `<topic>-<YYYY-MM-DD>.md`.
- A research file records what was established, by which method, which options were rejected and which experiment would tell the hypotheses apart. Current product behaviour is described elsewhere and wins over research.
- Raw evidence (measurement tables, JSON dumps) goes next to the research and is linked from it; the text keeps only the conclusions.

### adr/ — decisions

One decision — one file `NNNN-<topic>.md`, with a four-digit number in order of appearance.

- Contents: status (`accepted`, or `superseded by ADR-NNNN`), date, context, the decision, consequences, rejected alternatives. When the decision was given by a person, quote their words verbatim with the date and mark the quote as such; an agent's conclusion is not a decision.
- End with a section **«Where applied»**: links to the documents that rely on the decision.
- An ADR is **not rewritten** to match the current state. A new decision that cancels an old one is a new ADR; the old one changes only its status.
- Topic documents **link to the ADR instead of repeating the decision**.
- `adr/Index.md` is a table: number, date, one-line decision.

### features/<name>/ — features

A feature is what the product can do from the user's point of view: «search», «indexing», «window». It is the specification of the product.

- `Index.md` explains the feature in a few lines and leads to the other files.
- `requirements.md` holds the feature's requirements, referring to the project-wide requirement numbers instead of copying their text.
- `plan.md` lives while the feature is being built and is shortened or deleted when the feature is done.
- A feature document starts with the current state (accepted, in progress, paused, not accepted) and separates owner requirements from engineering choices of the implementation.
- Include a short **manual check list** when the feature has to be accepted by a person: what to do, what to see.

### chats/ — session journal

A session the user talks to keeps a folder `docs/chats/<YYYY-MM-DD>-<topic>/`. The topic is 2–4 words in Latin letters joined by hyphens. Subagents do not keep a journal: their result goes to the journal of the main session.

- **Start of a session**: find the latest `handoff.md` on your topic (through bars or by folder names) and read only it. Open `log.md` only when you need the detailed history.
- **`log.md` — only for a reason**: a non-obvious finding that affects further work, an approach that did not work (what was tried and why abandoned), a change of plan, a risky step (deletion, migration, rewriting many files). At most three lines per entry, appended to the end of the file without reading it (for example with `>>`). Do not write the course of the work, code, command output or what the diff shows. A decision with reasons goes to `adr/`, a durable finding to `knowledge/`; the log keeps one line with a link.
- **`handoff.md` — at the end of a session**, and on large milestones of a long one. Not longer than a screen. Sections: **Done** (what is ready and where it lies: files, commits), **Not done** (what is left of the task), **Difficulties** (what failed, what took the time, what is broken), **Next** (where the next session starts).
- The journal is history, not a description of the product. Search the `chats` module only when looking for what was done before and why.
- No keys, tokens, passwords or personal data in the journal, even in command output.

## Checklist before finishing a change to documentation

1. Every new or changed file has a `title`, one `#` heading and ATX headings only.
2. Each section is understandable without its neighbours; the headings name the subject.
3. The folder's `Index.md` lists the file; moved files have all links fixed.
4. Nothing is copied from another document: there is a link or a number instead.
5. Outdated text is edited or deleted, not left with a correction below.
6. No secrets.
