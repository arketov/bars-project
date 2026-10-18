import { afterEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import ResultCard from "./ResultCard.svelte";
import type { ScoreKind, SearchHit } from "./types";

vi.mock("./app.svelte", () => ({ app: { viewer: null } }));
vi.mock("./api", () => ({ copyText: vi.fn() }));

const hit = (score: number, score_kind: ScoreKind): SearchHit => ({
  path: "docs/search.md", module: "features", title: "Search",
  heading_chain: "Search/Threshold", first_line: 1, last_line: 3,
  text: "Search result", score, score_kind,
});

const components: ReturnType<typeof mount>[] = [];
function show(target: HTMLElement, score: number, kind: ScoreKind, rank: number) {
  components.push(mount(ResultCard, { target, props: { hit: hit(score, kind), rank } }));
  flushSync();
}
afterEach(() => { components.splice(0).forEach(component => void unmount(component)); });

describe("ResultCard score bars", () => {
  it("renders actual cosine values even when hybrid order differs", () => {
    const host = document.createElement("div");
    [0.63, 0.60, 0.62].forEach((score, i) => show(host, score, "cosine", i + 1));
    const meters = Array.from(host.querySelectorAll('[role="meter"]'));
    expect(meters.map(m => m.getAttribute("aria-valuenow"))).toEqual(["63", "60", "62"]);
    expect(meters.map(m => (m.firstElementChild as HTMLElement).style.width)).toEqual(["63%", "60%", "62%"]);
    expect(meters.every(m => m.getAttribute("aria-label") === "Косинусная близость")).toBe(true);
    expect(host.textContent).not.toMatch(/частично|хорошо подходит|слабо/);
    expect(host.querySelector(".fit")?.getAttribute("title")).toContain("близость может возрастать");
  });

  it("labels rerank and its cosine fallback by the actual hit kind", () => {
    for (const [kind, label] of [["rerank", "Оценка rerank"], ["cosine", "Косинусная близость"]] as const) {
      const host = document.createElement("div");
      show(host, 0.18, kind, 1);
      expect(host.querySelector('[role="meter"]')?.getAttribute("aria-label")).toBe(label);
      expect(host.querySelector('[role="meter"]')?.getAttribute("aria-valuenow")).toBe("18");
    }
  });
});
