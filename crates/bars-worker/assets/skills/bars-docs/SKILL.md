---
name: bars-docs
description: How to keep the documentation of a project indexed by bars: docs layout, file format, ADRs, feature lifecycle, Index.md files. Use when you create, move or edit documents in docs/.
---

# Documentation for bars

bars splits Markdown by headings and searches by section, so the shape of a document affects search quality. The full rules are in `BARS_INSTRUCTION.md` in the documentation root; read it before a larger change. Write documents in the language the project uses.

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

- `Index.md` in every folder except session folders: back-link to the parent and a table «file — when to read» listing every file and subfolder. Change it in the same change as the files.
- Module — a first-level subfolder; documents of one area go into one module.
- `knowledge/` — research, measurements, code conventions; write when a finding outlives the session. Snapshot research gets a date in its name.
- `adr/` — one decision per file `NNNN-<topic>.md`: status, date, context, decision (a person's words verbatim, with the date), consequences, rejected alternatives, «Where applied». An ADR is never rewritten: a new decision is a new ADR, the old one changes only its status. Topic documents link to the ADR instead of repeating it.
- `features/<name>/` — a feature as the user sees it: `Index.md` explains it and leads on, `requirements.md` holds requirements, `plan.md` lives while the feature is built, then shrink or delete it.
- `chats/` — session journal (skill `bars-session-log`).

## Before editing

Search the documentation with bars first (`bars_rag_search`) so you do not duplicate an existing document. Change the document first, then the code.
