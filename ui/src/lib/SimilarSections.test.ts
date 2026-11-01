import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, tick, unmount } from "svelte";
import SimilarSections from "./SimilarSections.svelte";
import { BarsError, type SearchHit, type SimilarOutput } from "./types";

const { rpc } = vi.hoisted(() => ({ rpc: vi.fn() }));
vi.mock("./api", () => ({ call: rpc }));
const hit = (id: string, score = 0.75): SearchHit => ({ chunk_id: id, path: `${id}.md`, title: id,
  heading_chain: `Раздел/${id}`, first_line: 12, last_line: 24, module: "features", text: `Текст ${id}`, score, score_kind: "cosine" });
const output = (id = "source"): SimilarOutput => ({ source: hit(id, 1), results: [hit("near", 0.9), hit("far", 0.5)], limit: 5, clamped: false, indexing: false, pending: 0 });
let host: HTMLDivElement, component: ReturnType<typeof mount> | undefined;
const onopen = vi.fn(), onrefresh = vi.fn();
const settle = async () => { await tick(); await Promise.resolve(); flushSync(); await tick(); };
function show() {
  component = mount(SimilarSections, { target: host, props: { project: "C:/project", sourceId: "source", modules: [{ name: "features", documents: 2 }, { name: ".", documents: 1 }], onopen, onrefresh } });
  flushSync();
}
function click(text: string) {
  const button = [...host.querySelectorAll("button")].find(b => b.textContent?.trim() === text);
  expect(button, text).toBeTruthy(); button!.click(); flushSync();
}
function number(label: string, value: string) {
  const input = host.querySelector<HTMLInputElement>(`[aria-label="${label}"]`)!;
  input.value = value; input.dispatchEvent(new Event("input", { bubbles: true })); flushSync();
}
beforeEach(() => { host = document.createElement("div"); document.body.append(host); vi.clearAllMocks(); rpc.mockResolvedValue(output()); });
afterEach(async () => { if (component) await unmount(component); component = undefined; host.remove(); });

it("uses the source project and ID; defaults are the entire project and worker limit", async () => {
  show(); await settle();
  expect(rpc).toHaveBeenCalledExactlyOnceWith("similar", { cwd: "C:/project", project: "C:/project", chunk_id: "source", modules: [] });
  expect(host.textContent).toContain("Исходный отрывок");
  expect(host.querySelectorAll("article")).toHaveLength(2);
  click("Открыть документ"); expect(onopen).toHaveBeenCalledWith(hit("near", 0.9));
  expect(host.textContent).toContain("оценка по близости");
});
it("continues from a neighbor and returns to the original source", async () => {
  show(); await settle(); click("Похожие"); await settle();
  expect(rpc.mock.lastCall?.[1].chunk_id).toBe("near");
  click("К исходному отрывку"); await settle(); expect(rpc.mock.lastCall?.[1].chunk_id).toBe("source");
});
it("changes candidate modules without borrowing the main search file scope", async () => {
  show(); await settle();
  host.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click(); flushSync(); await settle();
  expect(rpc.mock.lastCall?.[1].modules).toEqual(["features"]);
  click("Весь проект"); await settle(); expect(rpc.mock.lastCall?.[1].modules).toEqual([]);
});
it("filters the returned neighbors without an additional model or worker call", async () => {
  show(); await settle(); number("Порог похожих разделов", "0.8"); await settle();
  expect(host.querySelectorAll("article")).toHaveLength(1); expect(rpc).toHaveBeenCalledTimes(1);
  number("Порог похожих разделов", "1"); await settle(); expect(host.textContent).toContain("Нет соседей выше");
  number("Порог похожих разделов", "2"); expect(host.textContent).toContain("Порог — число от 0 до 1");
});
it("passes the requested limit and shows the actual clamped limit and indexing", async () => {
  show(); await settle(); rpc.mockResolvedValue({ ...output(), clamped: true, indexing: true, pending: 3 });
  number("Лимит похожих разделов", "100"); await settle();
  expect(rpc.mock.lastCall?.[1].limit).toBe(100);
  expect(host.textContent).toContain("лимит 5 (ограничен настройками)"); expect(host.textContent).toContain("В очереди: 3");
  number("Лимит похожих разделов", "0"); expect(host.textContent).toContain("Лимит — целое число"); expect(rpc).toHaveBeenCalledTimes(2);
});
it("distinguishes no other chunks from filtering", async () => {
  rpc.mockResolvedValue({ ...output(), results: [] }); show(); await settle();
  expect(host.textContent).toContain("Других разделов в выбранной области нет"); expect(host.textContent).not.toContain("Нет соседей выше");
});
it("offers a fresh search for an expired ID without replacing it automatically", async () => {
  rpc.mockRejectedValue(new BarsError("stale", "chunk_not_found")); show(); await settle();
  expect(host.textContent).toContain("Раздел изменился в индексе");
  click("Обновить поиск"); expect(onrefresh).toHaveBeenCalledTimes(1); expect(rpc).toHaveBeenCalledTimes(1);
});
it("shows worker failures and retries only on request", async () => {
  rpc.mockRejectedValue(new BarsError("Индекс недоступен", "reindex_required")); show(); await settle();
  expect(host.textContent).toContain("Индекс недоступен"); rpc.mockResolvedValue(output());
  click("Повторить"); await settle(); expect(host.querySelectorAll("article")).toHaveLength(2);
});
it("discards an old reply after the scope changes", async () => {
  let finish!: (out: SimilarOutput) => void;
  rpc.mockImplementationOnce(() => new Promise(resolve => { finish = resolve; })); show();
  host.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click(); flushSync(); await settle();
  finish({ ...output("old"), results: [hit("old-result")] }); await settle();
  expect(host.textContent).not.toContain("old-result"); expect(host.querySelectorAll("article")).toHaveLength(2);
});
it("discards completion after closing the panel", async () => {
  let finish!: (out: SimilarOutput) => void;
  rpc.mockImplementationOnce(() => new Promise(resolve => { finish = resolve; })); show();
  await unmount(component!); component = undefined; finish(output()); await settle(); expect(host.textContent).toBe("");
});
