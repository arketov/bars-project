import { expect, it } from "vitest";
import cases from "../../../crates/bars-graph/tests/fixtures/document-links.json";
import { renderDocument, resolveDocumentLink } from "./markdown";
import { resolveWikilink } from "./link-picker";

for (const sample of cases) it(`общий с Rust набор: ${sample.name}`, () => {
  const container = document.createElement("div"); container.innerHTML = renderDocument(sample.text);
  const targets = sample.paths.map(docs => ({docs, path: docs, title: docs}));
  const actual = [...container.querySelectorAll<HTMLElement>("a[data-doc-link], a[data-wiki-link]")].flatMap(link => {
    const path = link.dataset.wikiLink !== undefined ? resolveWikilink(targets,sample.source,link.dataset.wikiLink)?.path : resolveDocumentLink(sample.source,link.dataset.docLink ?? "");
    return path && sample.paths.includes(path) ? [path] : [];
  });
  expect(actual.sort()).toEqual([...sample.targets].sort());
});
