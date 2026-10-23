// Правила операций стенда 2.3.1 (data.ts): уровни, путь по карте, локальный граф, смысловой путь, влияние.
import { describe, expect, it } from "vitest";
import { CHUNK_LIMIT, F, XStore, adjacency, buildLevel, contextFor, impactOf, localGraph, mapPaths, semanticPaths, type LevelOptions, type XEdge } from "./data";

const opts = (o: Partial<LevelOptions> = {}): LevelOptions => ({ level: "documents", expanded: {}, scenario: "", chunkLimit: CHUNK_LIMIT, showUnresolved: false, topics: null, ...o });
const edge = (id: number, a: number, b: number, kind: XEdge["kind"] = "markdown_link", s = 1): XEdge => ({ id, key: `${kind}:${a}:${b}`, kind, a, b, raw: null, s, lines: [] });

describe("уровни одного снимка", () => {
  it("индексы узлов устойчивы между уровнями: документ уровня «Документы» — тот же узел в смешанном", () => {
    const store = new XStore();
    const docs = buildLevel(store, opts());
    const mixed = buildLevel(store, opts({ level: "mixed" }));
    expect(docs.visible.length).toBe(F.docs.length);
    for (const i of docs.visible) expect(mixed.visible).toContain(i);
    expect(mixed.edges.some((e) => e.kind === "mention")).toBe(true);
    expect(mixed.edges.filter((e) => e.kind === "assertion").length).toBe(F.assertions.length);
  });

  it("уровень «Чанки» усечён пределом с причиной и даёт группы документов", () => {
    const l = buildLevel(new XStore(), opts({ level: "chunks" }));
    expect(F.chunks.length).toBeGreaterThan(CHUNK_LIMIT);
    expect(l.visible.length).toBe(CHUNK_LIMIT);
    expect(l.truncated).toBe(true);
    expect(l.limitReasons[0]).toContain(`из ${F.chunks.length}`);
    expect(l.hulls.length).toBeGreaterThan(10);
  });

  it("уровень сущностей помечен экспериментальным, сценарий «пусто» не даёт узлов", () => {
    expect(buildLevel(new XStore(), opts({ level: "entities" })).banner[0]).toContain("экспериментальный");
    expect(buildLevel(new XStore(), opts({ scenario: "empty" })).visible.length).toBe(0);
  });

  it("неразрешённые ссылки — призрачные узлы, а не придуманные документы", () => {
    const store = new XStore();
    const l = buildLevel(store, opts({ showUnresolved: true }));
    const ghosts = l.visible.filter((i) => store.nodes[i].kind === "ghost");
    expect(ghosts.length).toBeGreaterThan(0);
    expect(ghosts.every((i) => !store.nodes[i].id.startsWith("doc:"))).toBe(true);
  });

  it("обзор тем агрегирует связи между свёрнутыми темами с числом", () => {
    const store = new XStore();
    const t = buildLevel(store, opts({ topics: new Set() }));
    expect(t.visible.every((i) => store.nodes[i].kind === "topic")).toBe(true);
    const agg = t.edges.filter((e) => e.kind === "aggregate");
    expect(agg.length).toBeGreaterThan(0);
    expect(agg.every((e) => Object.values(e.count ?? {}).reduce((a, b) => a + b, 0) > 0)).toBe(true);
  });
});

describe("путь по карте", () => {
  // 0-1-2-3 и обход 0-4-3; ребро 5-3 только в одну сторону.
  const edges = [edge(0, 0, 1), edge(1, 1, 2), edge(2, 2, 3), edge(3, 0, 4), edge(4, 4, 3, "document_similarity", 0.9), edge(5, 5, 3)];
  it("кратчайший первым, до трёх простых вариантов без повторов", () => {
    const paths = mapPaths(adjacency(edges), 0, 3);
    expect(paths[0].nodes).toEqual([0, 4, 3]);
    expect(paths[1].nodes).toEqual([0, 1, 2, 3]);
    expect(new Set(paths.map((p) => p.nodes.join())).size).toBe(paths.length);
  });
  it("по направлению ссылок не идёт против стрелки", () => {
    expect(mapPaths(adjacency(edges, true), 3, 5)).toEqual([]);
    expect(mapPaths(adjacency(edges, false), 3, 5)[0].nodes).toEqual([3, 5]);
  });
  it("не длиннее предела шагов", () => {
    expect(mapPaths(adjacency(edges), 0, 3, 3, 1)).toEqual([]);
  });
});

describe("локальный граф", () => {
  const star = Array.from({ length: 30 }, (_, i) => edge(i, 0, i + 1));
  it("глубина и предел обхода с честным признаком усечения", () => {
    const full = localGraph(adjacency(star), 0, 1, 200);
    expect(full.depthOf.size).toBe(31);
    expect(full.truncated).toBe(false);
    const cut = localGraph(adjacency(star), 0, 1, 10);
    expect(cut.depthOf.size).toBe(10);
    expect(cut.truncated).toBe(true);
    expect(cut.known).toBe(31);
  });
});

describe("операции воркера по снимку", () => {
  it("смысловой путь без цепочек пуст с оговоркой, а не «связи нет»", () => {
    const r = semanticPaths("doc:0", `doc:${F.docs.length - 1}`, "semantic");
    expect(r.paths).toEqual([]);
    expect(r.reason).toContain("не доказывает");
  });
  it("путь со структурными шагами находит цепочку через ссылки и упоминания", () => {
    const [a, b] = F.links[0];
    const r = semanticPaths(`doc:${a}`, `doc:${b}`, "mixed");
    expect(r.paths[0].steps[0].kind).toBe("link");
  });
  it("влияние отделяет ссылки-соседей от зависимостей", () => {
    const hub = F.links.reduce((m, [x, y]) => ((m[y] = (m[y] ?? 0) + 1), m), {} as Record<number, number>);
    const doc = Number(Object.entries(hub).sort((x, y) => y[1] - x[1])[0][0]);
    const r = impactOf(`doc:${doc}`);
    expect(r.neighbours.length).toBeGreaterThan(0);
    expect([...r.direct, ...r.indirect].every((x) => x.path.length > 0)).toBe(true);
  });
  it("контекст берёт записанный ответ с ролями и непокрытыми ролями", () => {
    const c = contextFor("правила установки в клиент");
    expect(c.groups.length).toBeGreaterThan(0);
    expect(Array.isArray(c.missing)).toBe(true);
  });
});
