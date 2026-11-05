import { afterEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import ResultCard from "./ResultCard.svelte";
import { groupHits } from "./search-results";
import type { ScoreKind, SearchHit } from "./types";

const { state } = vi.hoisted(() => ({ state: { showScores: false } }));
vi.mock("./app.svelte", () => ({ app: { viewer: null, get showScores() { return state.showScores; }, results: { applied: { signals: ["rerank", "fulltext", "exact"] } }, openDocument: vi.fn() } }));
vi.mock("./api", () => ({ copyText: vi.fn() }));

const hit = (first: number, score: number, kind: ScoreKind = "rerank"): SearchHit => ({
  path: "docs/search.md", module: "features", title: "Search", heading_chain: `Search/Раздел ${first}`, first_line: first, last_line: first + 2,
  text: "Порог результата поиска", score, score_kind: kind, signals: { rerank: score, fulltext: 0.2, exact: 0 },
});
const marker = { words: ["порог"], phrases: [] };
const components: ReturnType<typeof mount>[] = [];
function show(hits: SearchHit[]) {
  const target = document.createElement("div");
  components.push(mount(ResultCard, { target, props: { card: groupHits(hits)[0], marker } }));
  flushSync();
  return target;
}
afterEach(() => { components.splice(0).forEach(component => void unmount(component)); state.showScores = false; });

describe("карточка документа", () => {
  it("видны два раздела, остальные за «Ещё N разделов», лучший раскрыт", () => {
    const host = show([hit(1, 0.9), hit(10, 0.7), hit(20, 0.5), hit(30, 0.4)]);
    expect(host.querySelectorAll(".sec")).toHaveLength(2);
    expect(host.querySelectorAll(".sec.open")).toHaveLength(1);
    const more = host.querySelector<HTMLButtonElement>(".more")!;
    expect(more.textContent).toContain("Ещё 2 раздела");
    more.click(); flushSync();
    expect(host.querySelectorAll(".sec")).toHaveLength(4);
  });
  it("слова запроса подсвечены в отрывке, служебного по умолчанию нет", () => {
    const host = show([hit(1, 0.9), hit(10, 0.7)]);
    expect(host.querySelector(".sec:not(.open) mark")?.textContent).toBe("Порог");
    expect(host.querySelector(".score")).toBeNull();
    expect(host.querySelector(".sig")).toBeNull();
  });
  it("«показать баллы» добавляет число и разбор по сигналам; score_kind none — без оценки", () => {
    state.showScores = true;
    const host = show([hit(1, 0.9)]);
    expect(host.querySelector(".score")?.textContent).toContain("0,90");
    expect(host.querySelector(".sig")?.textContent).toContain("слова запроса");
    expect(host.querySelector(".sig")?.textContent).toContain("не используется");
    const none = show([hit(1, 0, "none")]);
    expect(none.querySelector(".score")?.textContent).toBe("—");
  });
});
