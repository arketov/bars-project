import { afterEach, expect, it, vi } from "vitest";
import { flushSync, mount, tick, unmount } from "svelte";
import ResultCard from "./ResultCard.svelte";
import { groupHits } from "./search-results";
import type { SearchHit } from "./types";

const { rpc, open } = vi.hoisted(() => ({ rpc: vi.fn(), open: vi.fn() }));
vi.mock("./app.svelte", () => ({ app: { viewer: null, showScores: false, results: null, current: "C:/project", displayed: { project: "C:/project" }, project: { modules: [] }, openDocument: open } }));
vi.mock("./api", () => ({ call: rpc, copyText: vi.fn() }));
const source: SearchHit = { chunk_id: "1-12345678", path: "docs/a.md", module: "features", title: "А", heading_chain: "А/Раздел", first_line: 3, last_line: 9, text: "Исходник", score: 0.8, score_kind: "cosine" };
let component: ReturnType<typeof mount>, target: HTMLDivElement;
const settle = async () => { await tick(); await Promise.resolve(); flushSync(); await tick(); };
afterEach(async () => { await unmount(component); target.remove(); vi.clearAllMocks(); });

it("opens similar from the search card and opens its neighbor at the returned range", async () => {
  const neighbor = { ...source, chunk_id: "2-12345678", path: "docs/b.md", first_line: 20, last_line: 30 };
  rpc.mockResolvedValue({ source, results: [neighbor], limit: 5, clamped: false, indexing: false, pending: 0 });
  target = document.createElement("div"); document.body.append(target);
  component = mount(ResultCard, { target, props: { card: groupHits([source])[0], marker: { words: [], phrases: [] } } }); flushSync();
  const entry = [...target.querySelectorAll("button")].find(b => b.textContent === "Похожие разделы")!;
  entry.click(); flushSync(); await settle();
  expect(rpc.mock.lastCall).toEqual(["similar", { cwd: "C:/project", project: "C:/project", chunk_id: source.chunk_id, modules: [] }]);
  target.querySelector<HTMLButtonElement>("[aria-label=\"Похожие разделы\"] article button")!.click(); expect(open).toHaveBeenCalledWith(neighbor);
  entry.click(); flushSync(); expect(target.querySelector('[aria-label="Похожие разделы"]')).toBeNull();
});
it("explains the unavailable action for a result from an old worker without a chunk ID", () => {
  target = document.createElement("div"); document.body.append(target);
  component = mount(ResultCard, { target, props: { card: groupHits([{ ...source, chunk_id: undefined }])[0], marker: { words: [], phrases: [] } } }); flushSync();
  const button = [...target.querySelectorAll("button")].find(b => b.textContent === "Похожие разделы")!;
  expect(button.disabled).toBe(true); expect(button.title).toContain("ID раздела отсутствует"); expect(rpc).not.toHaveBeenCalled();
});
