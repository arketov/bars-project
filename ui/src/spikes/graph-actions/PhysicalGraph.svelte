<script lang="ts">
  // Панель графа — стенд дизайна 2.3.1 (docs/features/graphrag/graph-actions-design.md). Подменяет продуктовую
  // lib/graph/PhysicalGraph.svelte и GraphPage только в конфигурации vite.graph-actions.config.ts. Основа — продуктовая панель
  // 22-10-2026 (2.1.5–2.2.3 и незавершённый 2.1.6); добавлены уровни, инспектор оснований, временные виды, подграф,
  // фильтры, список с вкладками, клавиатура и сценарии состояний GR-05. Данные — записанный снимок (data.ts), не воркер.
  import { onDestroy, onMount } from "svelte";
  import Icon from "../../lib/Icon.svelte";
  import { app } from "../../lib/app.svelte";
  import type { SearchHit } from "../../lib/types";
  import { DEFAULT_FORCES, Layout, type Forces, type PNode } from "../../lib/graph/layout";
  import { brightnessOf, drawStrength, springOf, widthOf, DEFAULT_MIN_STRENGTH, type GEdge, type GNode, type Mode } from "../../lib/graph/model";
  import { INITIAL_LIMITS, PORTION, type Slice } from "../../lib/graph/slice";
  import { DEFAULT_DEPTH, MAX_DEPTH, QUERY_BUDGET, QUERY_MAX, buildQuerySlice, shapeQuery, type QueryInfo, type QueryInput, type RetrievalKind, type ScoreKind } from "../../lib/graph/query";
  import { draw, emptyOverlay, layoutLines, lineDistance, moduleColors, radiusOf, readPalette, toScreen, toWorld, type Camera, type Overlay, type Palette, type Scene, type Target } from "./draw";
  import {
    AUDIT_NAME, CHUNK_LIMIT, CHUNK_MAX, F, GROUP_NAME, LEVELS, LEVEL_NAME, PREDICATE_GROUP, PREDICATE_NAME, ROLE_LETTER, ROLE_NAME, SCENARIO_TARGETS, XKIND_TITLE, XStore,
    adjacency, buildLevel, contextFor, evidenceOf, hubs, impactOf, localGraph, mapPaths, semanticPaths,
    type ContextReply, type ImpactResult, type Level, type LevelSlice, type LocalResult, type MapPath, type Scenario, type SemResult, type XEdge, type XKind, type XNode,
  } from "./data";

  const P = new URLSearchParams(location.search);
  const scenario = (P.get("scenario") ?? "") as Scenario;
  const reduced = P.get("motion") === "reduced" || (typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches);

  let host: HTMLDivElement;
  let stage: HTMLDivElement;
  let canvas = $state<HTMLCanvasElement | null>(null);
  let mini = $state(false);
  let mounted = $state(false);
  let size = $state({ w: 0, h: 0 });

  // ---------- Уровень, режим, фильтры
  let settings = $state({ ...defaultPanel(), mode: (["links", "meaning", "all"].includes(P.get("mode") ?? "") ? P.get("mode") : "links") as Mode });
  function defaultPanel() {
    return { forces: { ...DEFAULT_FORCES } as Forces, minS: DEFAULT_MIN_STRENGTH, mode: "links" as Mode };
  }
  const mode = $derived(settings.mode);
  const minS = $derived(settings.minS);
  const forces = $derived(settings.forces);
  let level = $state<Level>((LEVELS as string[]).includes(P.get("level") ?? "") ? (P.get("level") as Level) : "documents");
  const lvlOf = () => (mini ? "documents" : level);

  interface Filters {
    predicates: Set<string>;
    explicit: boolean;
    candidate: boolean;
    conditions: "all" | "with" | "without";
    negations: boolean;
    direction: "both" | "in" | "out";
    unresolved: boolean;
    nodeKinds: { document: boolean; chunk: boolean; entity: boolean };
  }
  const ALL_PREDICATES = [...new Set(F.assertions.map((a) => a.predicate))].sort();
  const freshFilters = (): Filters => ({ predicates: new Set(ALL_PREDICATES), explicit: true, candidate: true, conditions: "all", negations: true, direction: "both", unresolved: scenario === "unresolved", nodeKinds: { document: true, chunk: true, entity: true } });
  let filters = $state<Filters>(freshFilters());
  const activeFilters = $derived(
    (filters.predicates.size < ALL_PREDICATES.length ? 1 : 0) + (!filters.explicit || !filters.candidate ? 1 : 0) + (filters.conditions !== "all" ? 1 : 0) + (!filters.negations ? 1 : 0) +
      (filters.direction !== "both" ? 1 : 0) + (filters.unresolved && scenario !== "unresolved" ? 1 : 0) + (Object.values(filters.nodeKinds).some((v) => !v) ? 1 : 0),
  );

  /** Виды рёбер, видимые на уровне в режиме. */
  const kinds = $derived.by(() => {
    const k = {} as Record<XKind, boolean>;
    const l = lvlOf();
    const m = mode;
    const links = m === "links" || m === "all";
    const sense = m === "meaning" || m === "all";
    k.query_result = true;
    k.unresolved = filters.unresolved || scenario === "unresolved" || tv?.kind === "audit";
    k.aggregate = tv?.kind === "topics";
    if (l === "documents" || l === "mixed") {
      k.markdown_link = links;
      k.document_similarity = sense;
      k.chunk_similarity = m === "chunks" || m === "all";
    }
    if (l === "documents" && m === "chunks") k.chunk_similarity = true;
    if (l === "chunks") {
      k.markdown_link = links;
      k.chunk_similarity = sense;
      k.sequence = true;
    }
    if (l === "entities" || l === "mixed") k.assertion = true;
    if (l === "mixed") k.mention = true;
    if (tv?.kind === "topics") k.markdown_link = true;
    return k;
  });

  // ---------- Данные
  const xs = new XStore();
  let expanded = $state<Record<number, number>>({});
  const shownChunks = $derived(Object.values(expanded).reduce((a, b) => a + b, 0));
  let chunkLimit = $state(CHUNK_LIMIT);
  const EMPTY_LEVEL: LevelSlice = { visible: [], edges: [], hulls: [], totals: { nodes: 0, shown: 0 }, truncated: false, limitReasons: [], layers: { links: "ready", meaning: "ready", semantic: "partial" }, banner: [] };
  let lvl = $state.raw<LevelSlice>(EMPTY_LEVEL);
  let gen = $state.raw<Slice>(toSlice(EMPTY_LEVEL));
  let slice = $state.raw<Slice>(toSlice(EMPTY_LEVEL));
  let loaded = $state(false);
  const loadError = scenario === "notbuilt" ? "граф ещё не построен: слой связей «не построен»" : "";

  function toSlice(l: LevelSlice): Slice {
    const visibleDocs = new Set(l.visible.filter((i) => xs.nodes[i]?.kind === "document"));
    const chunksOf = new Map<number, number[]>();
    for (const i of l.visible) {
      const n = xs.nodes[i];
      if (n?.kind === "chunk") chunksOf.set(n.owner, [...(chunksOf.get(n.owner) ?? []), i]);
    }
    const layer = (state: string, note: string) => ({ state: state as never, note });
    return {
      nodes: xs.nodes as unknown as GNode[],
      docCount: visibleDocs.size,
      edges: l.edges as unknown as GEdge[],
      chunksOf,
      visibleDocs,
      totals: { documents: scenario === "empty" ? 0 : F.docs.length, chunks: F.chunks.length, links: F.links.length, document_pairs: F.docSims.length },
      truncated: l.truncated,
      limitReasons: l.limitReasons,
      warnings: [],
      layers: {
        markdown_link: layer(l.layers.links, l.layers.links === "ready" ? "готов" : "Слой ссылок не построен"),
        document_similarity: layer(l.layers.meaning, l.layers.meaning === "ready" ? "готов" : "Слой близости не построен"),
        chunk_similarity: layer(l.layers.meaning, l.layers.meaning === "ready" ? "готов" : "Слой близости не построен"),
      },
      snapshot: F.snapshot,
      applied: INITIAL_LIMITS,
      usedBytes: 0,
    };
  }

  function loadLevel() {
    lvl = buildLevel(xs, { level: lvlOf(), expanded, scenario, chunkLimit, showUnresolved: filters.unresolved || tv?.kind === "audit", topics: null });
    gen = toSlice(lvl);
    compose();
    loaded = true;
  }

  const nodeOf = (i: number) => slice.nodes[i] as unknown as XNode;
  const edgeOf = (id: number) => slice.edges[id] as unknown as XEdge;
  const owner = (index: number) => slice.nodes[index].owner;
  const label = (index: number) => slice.nodes[index]?.label ?? "";

  // ---------- Вид запроса (как в продукте 2.2.3); в стенде запрос — параметр адреса `query=`, результаты подбираются по словам названий
  let query = $state.raw<QueryInfo | null>(null);
  let queryDepth = $state(DEFAULT_DEPTH);
  let queryBudget = $state({ ...QUERY_BUDGET });
  let queryOff = $state("");
  const standQuery = P.get("query");
  function queryInput(): QueryInput | null {
    if (!standQuery || lvlOf() !== "documents") return null;
    const words = standQuery.toLowerCase().split(/\s+/).filter((w) => w.length > 2);
    const hits = F.docs
      .map((d, i) => ({ d, i, hit: words.filter((w) => (d[0] + " " + d[2]).toLowerCase().includes(w.slice(0, Math.max(4, w.length - 2)))).length }))
      .filter((x) => x.hit)
      .sort((a, b) => b.hit - a.hit)
      .slice(0, 8)
      .map((x, k) => ({ path: x.d[1], module: x.d[3], title: x.d[0], heading_chain: x.d[0], first_line: 1, last_line: 12, text: "", score: Math.round((0.92 - k * 0.07) * 100) / 100 }) as unknown as SearchHit);
    return { id: "stand-query", text: standQuery, hits, scoreKind: "rerank", requested: "hybrid", applied: "hybrid", fallback: null, minScore: 0.18 };
  }
  function compose() {
    const input = queryInput();
    if (input && input.id !== queryOff) {
      const built = buildQuerySlice(gen, input);
      slice = built.slice;
      query = built.info;
    } else {
      slice = gen;
      query = null;
    }
  }
  const modeKindsList = (m: Mode) => (m === "links" ? ["markdown_link"] : m === "meaning" ? ["document_similarity"] : m === "chunks" ? ["chunk_similarity"] : ["markdown_link", "document_similarity", "chunk_similarity"]);
  const queryShape = $derived.by(() => (query ? shapeQuery(slice, query, modeKindsList(mode) as never, minS, queryDepth, queryBudget) : null));
  function leaveQuery() {
    if (!query) return;
    queryOff = query.id;
    selected = null;
    compose();
    announce("Общий вид проекта");
    rebuild(0.6, true);
  }
  function enterQuery() {
    queryOff = "";
    selected = null;
    compose();
    rebuild(0.9, true);
  }
  const queryOffered = $derived(!query && !!queryOff && !!standQuery);
  const SCORE_TEXT: Record<ScoreKind, string> = { rerank: "оценка rerank", cosine: "оценка — косинус (rerank не выполнялся)", none: "оценки нет" };
  const SEARCH_MODE: Record<RetrievalKind, string> = { hybrid: "Обычный", mix: "Со связями", graph: "Отношения и пути" };

  // ---------- Временные виды (OQ-120 как образец): одна полоса, «Общий вид» возвращает места, камеру и выбор
  type TV =
    | { kind: "local"; center: number; depth: number; budget: number; res: LocalResult }
    | { kind: "pick"; from: number }
    | { kind: "path"; from: number; to: number; alts: MapPath[]; at: number; directed: boolean; sem: SemResult | null; semKind: "semantic" | "mixed" }
    | { kind: "impact"; root: number; res: ImpactResult }
    | { kind: "context"; reply: ContextReply; task: string; profile: string; scope: string }
    | { kind: "topics"; open: Set<number> }
    | { kind: "audit" };
  let tv = $state.raw<TV | null>(null);
  /** Общий вид живёт в своей раскладке; временный вид с перекладкой получает отдельную, поэтому возврат точный. */
  let general: Layout | null = null;
  let layout = $state.raw<Layout | null>(null);
  let saved: { cam: Camera; selected: Target; w: number; h: number } | null = null;
  const VIEW_NAME: Record<TV["kind"], string> = { local: "Локальный граф", pick: "Путь", path: "Путь", impact: "Влияние изменения", context: "Контекст задачи", topics: "Обзор тем", audit: "Аудит связей" };

  let selected = $state<Target>(null);
  let tip = $state<{ text: string; x: number; y: number } | null>(null);
  let motion = $state<"running" | "idle" | "stopped" | "reduced">(reduced ? "reduced" : "idle");
  let stopped = $state(false);
  let view = $state<"map" | "list">(scenario === "norender" ? "list" : "map");
  let listTab = $state<"nodes" | "edges" | "view">("nodes");
  let listFilter = $state("");
  let findText = $state("");
  let legendOpen = $state(false);
  let popover = $state<"settings" | "filters" | "analysis" | "predicates" | null>(null);
  let notice = $state("");
  let rendererError = $state(scenario === "norender" ? "Canvas недоступен: карта заменена списком. Все действия работают из списка и инспектора." : "");
  let revision = $state(0);
  let subgraph = $state.raw<Set<number>>(new Set());
  let ctxForm = $state<{ task: string; profile: string; budget: number; useSubgraph: boolean } | null>(null);
  let scopeUndo = $state.raw<ReadonlySet<string> | null>(null);
  let toast = $state("");
  let toastTimer = 0;
  let pulse = 0;
  let pulseUntil = 0;

  let palette = $state.raw<Palette | null>(null);
  let cam: Camera = { x: 0, y: 0, k: 1 };
  let fitted = false;
  let autoFitSince = 0;
  let lastAutoFit = 0;
  const AUTO_FIT_MS = 30000;
  let camTarget: Camera | null = null;
  let raf = 0;
  let hidden = false;
  let hovered: Target = null;
  let hostObserver: ResizeObserver | null = null;
  let themeObserver: MutationObserver | null = null;

  // ---------- Видимое
  function nodeKindOn(n: XNode) {
    if (n.kind === "document" || n.kind === "ghost") return filters.nodeKinds.document;
    if (n.kind === "chunk") return filters.nodeKinds.chunk;
    if (n.kind === "entity") return filters.nodeKinds.entity;
    return true;
  }
  function visibleNodes(): GNode[] {
    if (queryShape) return queryShape.nodes.map((index) => slice.nodes[index]);
    if (tv?.kind === "local") return [...tv.res.depthOf.keys()].map((i) => slice.nodes[i]);
    if (tv?.kind === "topics") return topicsVisible.map((i) => slice.nodes[i]);
    return lvl.visible.filter((i) => nodeKindOn(xs.nodes[i])).map((i) => slice.nodes[i]);
  }
  const passes = (e: XEdge) => e.kind !== "document_similarity" && e.kind !== "chunk_similarity" ? true : e.s >= minS;
  function edgePass(e: XEdge): boolean {
    if (!kinds[e.kind] || !passes(e)) return false;
    if (e.kind === "assertion" && e.assertion !== undefined) {
      const a = F.assertions[e.assertion];
      if (!filters.predicates.has(a.predicate)) return false;
      const explicit = a.origin === "explicit_rule";
      if ((explicit && !filters.explicit) || (!explicit && !filters.candidate)) return false;
      const cond = hasCondition(e.assertion);
      if ((filters.conditions === "with" && !cond) || (filters.conditions === "without" && cond)) return false;
      if (!filters.negations && a.polarity === "negative") return false;
    }
    const centre = tv?.kind === "local" ? tv.center : selected?.type === "node" ? selected.index : null;
    if (filters.direction !== "both" && centre !== null && (e.kind === "markdown_link" || e.kind === "assertion")) {
      if (filters.direction === "out" && e.b === centre) return false;
      if (filters.direction === "in" && e.a === centre) return false;
    }
    return true;
  }
  const hasCondition = (k: number) => Object.keys(F.assertions[k].qualifiers).length > 0 || (scenario === "conditions" && k === SCENARIO_TARGETS.condition);

  function visibleEdges(nodes = visibleNodes()): GEdge[] {
    const set = new Set(nodes.map((n) => n.index));
    return (slice.edges as unknown as XEdge[]).filter(
      (e) => edgePass(e) && !(mini && e.kind === "chunk_similarity") && set.has(e.a) && set.has(e.b) && (!queryShape?.edgeIds || e.kind === "query_result" || queryShape.edgeIds.has(e.id)) && (tv?.kind !== "local" || tv.res.edges.has(e.id)),
    ) as unknown as GEdge[];
  }
  /** Сколько скрывают фильтры: узлы и связи уровня, которые не прошли фильтры при включённом режиме. */
  const hiddenByFilters = $derived.by(() => {
    void revision;
    if (!activeFilters) return null;
    const allNodes = lvl.visible.length;
    const shownNodes = lvl.visible.filter((i) => nodeKindOn(xs.nodes[i])).length;
    const modeEdges = lvl.edges.filter((e) => kinds[e.kind] && passes(e)).length;
    const shownEdges = lvl.edges.filter((e) => edgePass(e)).length;
    return { nodes: allNodes - shownNodes, edges: modeEdges - shownEdges };
  });

  function rebuild(reheat = 0.55, fresh = false) {
    if (!layout) return;
    const nodes = visibleNodes();
    layout.setGraph(nodes, visibleEdges(nodes), owner);
    if (fresh) {
      layout.reseed();
      if (reduced || stopped) layout.settle(600, true);
      else {
        layout.settle(40);
        layout.reheat(0.8);
      }
      autoFitSince = performance.now();
      lastAutoFit = 0;
    } else if (reduced) layout.settle(600, true);
    else if (!stopped) layout.reheat(reheat);
    revision++;
    schedule();
  }

  function schedule() {
    if (!raf && canvas) raf = requestAnimationFrame(frame);
  }
  function frame() {
    raf = 0;
    if (!layout || !canvas) return;
    if (camTarget) {
      const t = camTarget;
      const near = Math.abs(t.k - cam.k) < 0.002 * t.k && Math.abs(t.x - cam.x) < 0.6 && Math.abs(t.y - cam.y) < 0.6;
      cam = near || reduced ? t : { k: cam.k + (t.k - cam.k) * 0.18, x: cam.x + (t.x - cam.x) * 0.18, y: cam.y + (t.y - cam.y) * 0.18 };
      if (near || reduced) camTarget = null;
      else schedule();
    }
    const now = performance.now();
    pulse = now < pulseUntil ? (Math.sin((now / 160) * Math.PI) + 1) / 2 : 0;
    if (now < pulseUntil) schedule();
    paint();
    const next = reduced ? "reduced" : stopped ? "stopped" : layout.active && !hidden ? "running" : "idle";
    if (next !== motion) motion = next;
  }
  function onPositions() {
    revision++;
    if (autoFitSince && size.w > 1 && layout?.nodes.length) {
      const now = performance.now();
      const idle = !layout.active || reduced || stopped;
      if (now - autoFitSince > AUTO_FIT_MS) autoFitSince = 0;
      else if (idle || now - lastAutoFit > 300) {
        lastAutoFit = now;
        fit(true);
        if (idle) autoFitSince = 0;
      }
    }
    schedule();
  }

  function related(): Set<number> {
    const set = new Set<number>();
    if (!layout || tv) return set;
    if (selected?.type === "node") {
      for (const l of layout.links) {
        if (l.source.index === selected.index) set.add(l.target.index);
        else if (l.target.index === selected.index) set.add(l.source.index);
      }
    } else if (selected?.type === "edge") {
      const l = layout.links.find((x) => x.edge.id === (selected as { id: number }).id);
      if (l) set.add(l.source.index).add(l.target.index);
    }
    return set;
  }
  function markedNodes(): Set<number> {
    const set = new Set<number>();
    const q = findText.trim().toLowerCase();
    if (!layout || !q) return set;
    for (const n of layout.nodes) if (n.node.label.toLowerCase().includes(q)) set.add(n.index);
    return set;
  }
  const moduleMap = $derived(moduleColors(F.docs.map((d) => d[3])));

  // ---------- Наложение временного вида
  const docOfEntity = (ent: number) => F.entities[ent].doc;
  /** Узел текущего уровня для объекта снимка: на уровне документов сущность показывается документом своего определения. */
  function nodeFor(id: string): number | undefined {
    const direct = xs.indexOf(id);
    if (direct !== undefined && lvl.visible.includes(direct)) return direct;
    if (id.startsWith("ent:")) {
      const d = docOfEntity(Number(id.slice(4)));
      if (d >= 0) return xs.indexOf(`doc:${d}`);
    }
    return direct;
  }
  function nodeForEvidence(evId: string): number | undefined {
    const ev = evidenceOf(evId);
    if (!ev) return undefined;
    if (lvlOf() === "chunks") {
      const ci = F.chunks.findIndex((c) => c[0] === ev.doc && c[1] <= ev.first && ev.first <= c[2]);
      if (ci >= 0) return xs.indexOf(`chunk:${ci}`);
    }
    return xs.indexOf(`doc:${ev.doc}`);
  }

  const overlay = $derived.by((): Overlay => {
    void revision;
    const o = emptyOverlay();
    o.subgraph = subgraph;
    if (lvlOf() === "chunks") o.hulls = lvl.hulls.map((h) => ({ label: h.label, members: h.members }));
    for (const e of slice.edges as unknown as XEdge[]) {
      if (e.kind === "assertion" && e.assertion !== undefined) {
        const a = F.assertions[e.assertion];
        o.edgeMeta.set(e.id, { neg: a.polarity === "negative", cond: hasCondition(e.assertion), conflict: e.conflict, candidate: a.origin !== "explicit_rule", label: PREDICATE_NAME[a.predicate] ?? a.predicate });
      } else if (e.kind === "aggregate" && e.count) o.edgeMeta.set(e.id, { count: Object.values(e.count).reduce((x, y) => x + y, 0) });
    }
    o.picking = tv?.kind === "pick";
    if (!tv) return o;
    if (tv.kind === "local") o.rings.set(tv.center, "center");
    if (tv.kind === "pick") o.rings.set(tv.from, "center");
    if (tv.kind === "path") {
      const p = tv.alts[tv.at];
      o.focusSet = new Set(p ? p.nodes : [tv.from, tv.to]);
      if (p) {
        p.nodes.forEach((n, i) => o.stepOf.set(n, i));
        for (const s of p.steps) o.pathEdges.add(s.edge.id);
      } else {
        o.stepOf.set(tv.from, 0);
        o.stepOf.set(tv.to, 1);
      }
    }
    if (tv.kind === "impact") {
      o.focusSet = new Set([tv.root]);
      o.rings.set(tv.root, "center");
      for (const n of tv.res.neighbours) {
        const i = xs.indexOf(`doc:${n.doc}`);
        if (i !== undefined) {
          o.rings.set(i, "neighbour");
          o.focusSet.add(i);
        }
      }
      for (const it of [...tv.res.direct, ...tv.res.indirect]) {
        const i = nodeFor(it.id);
        if (i === undefined) continue;
        o.rings.set(i, it.direct ? "direct" : "indirect");
        o.focusSet.add(i);
      }
    }
    if (tv.kind === "context") {
      o.focusSet = new Set();
      for (const g of tv.reply.groups)
        for (const ev of g.evidence) {
          const i = nodeForEvidence(ev);
          if (i === undefined) continue;
          o.focusSet.add(i);
          if (!o.badges.has(i)) o.badges.set(i, ROLE_LETTER[g.role] ?? "?");
        }
    }
    if (tv.kind === "audit") {
      o.focusSet = new Set();
      for (const a of F.audit) {
        if (a.doc === null || a.kind !== "structural_isolate") continue;
        const i = xs.indexOf(`doc:${a.doc}`);
        if (i !== undefined) {
          o.rings.set(i, "isolate");
          o.focusSet.add(i);
        }
      }
      for (const h of hubs()) {
        const i = xs.indexOf(`doc:${h.doc}`);
        if (i !== undefined) {
          o.rings.set(i, "hub");
          o.focusSet.add(i);
        }
      }
      for (const e of slice.edges as unknown as XEdge[]) if (e.kind === "unresolved") o.focusSet.add(e.a).add(e.b);
    }
    return o;
  });

  function paint() {
    if (!canvas || !layout || !palette || size.w < 2) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      rendererError = "Canvas недоступен: карта заменена списком.";
      view = "list";
      return;
    }
    const groups = lvlOf() === "documents" && !tv ? Object.entries(expanded).map(([o, shown]) => ({ owner: Number(o), shown, total: slice.nodes[Number(o)]?.chunks ?? shown })) : [];
    const scene: Scene = {
      w: size.w, h: size.h, dpr: window.devicePixelRatio || 1, cam, nodes: layout.nodes, links: layout.links, palette, kinds,
      selected, hovered, related: related(), marked: markedNodes(), groups, mini, modules: moduleMap, x: overlay, still: reduced, pulse,
    };
    draw(ctx, scene);
  }

  function fit(auto = false) {
    if (!auto) {
      autoFitSince = 0;
      camTarget = null;
    }
    if (!layout || size.w < 2) return;
    const b = layout.bounds();
    if (!b) return;
    const pad = mini ? 14 : 36;
    const k = Math.min(1.6, Math.max(0.12, Math.min((size.w - pad * 2) / Math.max(40, b.x1 - b.x0), (size.h - pad * 2) / Math.max(40, b.y1 - b.y0))));
    const target = { k, x: size.w / 2 - ((b.x0 + b.x1) / 2) * k, y: size.h / 2 - ((b.y0 + b.y1) / 2) * k };
    if (auto && fitted && !reduced) camTarget = target;
    else {
      cam = target;
      camTarget = null;
    }
    fitted = true;
    schedule();
  }
  /** Камера на набор узлов (путь, влияние): без перекладки. */
  function fitTo(indices: number[]) {
    if (!layout || size.w < 2 || !indices.length) return;
    const ps = indices.map((i) => layout!.get(i)).filter((p): p is PNode => !!p);
    if (!ps.length) return;
    const x0 = Math.min(...ps.map((p) => p.x)), x1 = Math.max(...ps.map((p) => p.x)), y0 = Math.min(...ps.map((p) => p.y)), y1 = Math.max(...ps.map((p) => p.y));
    const k = Math.min(1.8, Math.max(0.2, Math.min((size.w - 120) / Math.max(60, x1 - x0), (size.h - 120) / Math.max(60, y1 - y0))));
    const target = { k, x: size.w / 2 - ((x0 + x1) / 2) * k, y: size.h / 2 - ((y0 + y1) / 2) * k };
    autoFitSince = 0;
    if (reduced) cam = target;
    else camTarget = target;
    schedule();
  }
  function zoomAt(factor: number, sx = size.w / 2, sy = size.h / 2) {
    autoFitSince = 0;
    camTarget = null;
    const k = Math.min(6, Math.max(0.1, cam.k * factor));
    const [wx, wy] = toWorld(cam, sx, sy);
    cam = { k, x: sx - wx * k, y: sy - wy * k };
    schedule();
  }
  function setStopped(value: boolean) {
    stopped = value;
    if (value) layout?.cool();
    else layout?.reheat(0.3);
    motion = reduced ? "reduced" : value ? "stopped" : "idle";
    schedule();
  }

  // ---------- Раскрытие
  function toggleExpand(index: number) {
    if (mini || query || tv || lvlOf() !== "documents") return;
    const n = nodeOf(index);
    if (n.kind === "topic") return;
    if (n.kind !== "document" || !n.chunks) return;
    if (expanded[index]) {
      const { [index]: _, ...rest } = expanded;
      expanded = rest;
      if (mode === "chunks" && !Object.keys(rest).length) settings.mode = "meaning";
      if (selected?.type === "node" && nodeOf(selected.index).owner === index && selected.index !== index) selected = { type: "node", index };
    } else expanded = { ...expanded, [index]: Math.min(PORTION, n.chunks) };
    loadLevel();
    rebuild(0.5);
  }

  // ---------- Уровни и режимы
  const MODE_NAME: Record<Mode, string> = { links: "Ссылки", meaning: "По смыслу", chunks: "По смыслу · чанки", all: "Все связи" };
  const availableModes = $derived<Mode[]>(lvlOf() === "entities" ? [] : lvlOf() === "documents" && !mini && shownChunks > 0 ? ["links", "meaning", "chunks", "all"] : ["links", "meaning", "all"]);
  function announce(text: string) {
    toast = text;
    clearTimeout(toastTimer);
    toastTimer = window.setTimeout(() => (toast = ""), 2600);
  }
  function setMode(next: Mode, say = true) {
    if (next === mode || lvlOf() === "entities") return;
    settings.mode = next;
    if (selected?.type === "edge") selected = null;
    if (say) announce(`Режим: ${MODE_NAME[next].toLowerCase()}`);
    if (tv?.kind === "local") recomputeLocal();
    else if (tv?.kind === "path") recomputePath();
    rebuild(0.8, true);
  }
  function cycleMode() {
    const list = availableModes;
    if (list.length) setMode(list[(list.indexOf(mode) + 1) % list.length]);
  }
  function setLevel(next: Level) {
    if (next === level || mini) return;
    if (tv && tv.kind !== "context" && tv.kind !== "audit") closeView(true);
    const keep = selected?.type === "node" ? xs.nodes[selected.index] : null;
    level = next;
    if (next !== "documents" && mode === "chunks") settings.mode = "all";
    loadLevel();
    // Выбранный объект сохраняется, если он есть на новом уровне; документ на уровне чанков — как группа.
    if (keep && !lvl.visible.includes(keep.index)) selected = null;
    announce(`Уровень: ${LEVEL_NAME[next].toLowerCase()}`);
    rebuild(0.8, true);
  }
  function cycleLevel() {
    if (!mini) setLevel(LEVELS[(LEVELS.indexOf(level) + 1) % LEVELS.length]);
  }

  // ---------- Временные виды
  function startRelayoutView() {
    saved = { cam: { ...cam }, selected, w: size.w, h: size.h };
    general?.hold(true);
    const vl = new Layout();
    vl.onPositions = onPositions;
    vl.setForces($state.snapshot(settings.forces) as Forces);
    layout = vl;
  }
  function closeView(silent = false) {
    if (!tv) return;
    const relayout = tv.kind === "local" || tv.kind === "topics";
    const wasAudit = tv.kind === "audit";
    tv = null;
    if (relayout) {
      if (layout && layout !== general) layout.destroy();
      layout = general;
      general?.hold(false);
      if (saved) {
        // Автовписывание временного вида не должно перехватить возвращённую камеру.
        autoFitSince = 0;
        camTarget = null;
        // Камера запомнена при прежней высоте карты; полоса вида уходит, и ResizeObserver сдвинет её на половину разницы обратно.
        const back = saved;
        cam = { ...back.cam };
        selected = back.selected;
        setTimeout(() => {
          cam = { ...back.cam, x: back.cam.x + (size.w - back.w) / 2, y: back.cam.y + (size.h - back.h) / 2 };
          schedule();
        }, 80);
      }
      saved = null;
      compose();
    } else if (wasAudit) {
      loadLevel();
      rebuild(0.4);
    }
    if (!silent) announce("Общий вид");
    revision++;
    schedule();
  }

  function openLocal(center: number, depth = Number(P.get("depth")) || 1) {
    if (tv && tv.kind !== "local") closeView(true);
    const fresh = !tv;
    const budget = scenario === "limit" ? 6 : 200;
    tv = { kind: "local", center, depth, budget, res: computeLocal(center, depth, budget) };
    if (fresh) startRelayoutView();
    selected = { type: "node", index: center };
    rebuild(0.9, true);
    layout?.dragTo(center, 0, 0);
    layout?.dragEnd(center);
    announce(`Локальный граф «${label(center)}», глубина ${depth}`);
  }
  function computeLocal(center: number, depth: number, budget: number) {
    const nodes = lvl.visible.filter((i) => nodeKindOn(xs.nodes[i]));
    const set = new Set(nodes);
    const edges = (lvl.edges as XEdge[]).filter((e) => edgePass(e) && set.has(e.a) && set.has(e.b));
    return localGraph(adjacency(edges), center, depth, budget);
  }
  function recomputeLocal() {
    if (tv?.kind !== "local") return;
    tv = { ...tv, res: computeLocal(tv.center, tv.depth, tv.budget) };
  }
  function setLocalDepth(d: number) {
    if (tv?.kind !== "local") return;
    tv = { ...tv, depth: d, res: computeLocal(tv.center, d, tv.budget) };
    rebuild(0.5);
  }
  function moreLocal() {
    if (tv?.kind !== "local") return;
    const budget = Math.min(500, tv.budget * 2 < 200 ? 200 : tv.budget * 2);
    tv = { ...tv, budget, res: computeLocal(tv.center, tv.depth, budget) };
    notice = "Показано больше узлов локального графа: позиции сохранены.";
    rebuild(0.4);
  }
  function selectLocalAsSubgraph() {
    if (tv?.kind !== "local") return;
    subgraph = new Set([...subgraph, ...tv.res.depthOf.keys()]);
    announce(`В подграфе ${subgraph.size} узлов`);
  }

  function startPath(from: number) {
    if (tv && tv.kind !== "pick") closeView(true);
    tv = { kind: "pick", from };
    announce("Выберите вторую вершину: щелчком, в поиске узла или в списке");
  }
  function finishPath(to: number) {
    if (tv?.kind !== "pick" || to === tv.from) return;
    const from = tv.from;
    tv = { kind: "path", from, to, alts: [], at: 0, directed: false, sem: null, semKind: "semantic" };
    recomputePath();
    selected = null;
    const p = tv.kind === "path" ? tv.alts[0] : null;
    fitTo(p ? p.nodes : [from, to]);
    // Раскладка ещё движется: второе вписывание после того, как она устоится.
    setTimeout(() => tv?.kind === "path" && fitTo(tv.alts[tv.at]?.nodes ?? [tv.from, tv.to]), 1200);
    announce(p ? `Путь по карте: ${p.steps.length} шагов` : `В режиме «${MODE_NAME[mode]}» пути нет`);
  }
  function recomputePath() {
    if (tv?.kind !== "path") return;
    const nodes = new Set((layout?.nodes ?? []).map((n) => n.index));
    const edges = (lvl.edges as XEdge[]).filter((e) => edgePass(e) && nodes.has(e.a) && nodes.has(e.b) && e.kind !== "sequence");
    tv = { ...tv, alts: mapPaths(adjacency(edges, tv.directed), tv.from, tv.to, 3, 6), at: 0 };
    revision++;
  }
  function pathAlt(step: number) {
    if (tv?.kind !== "path" || !tv.alts.length) return;
    tv = { ...tv, at: (tv.at + step + tv.alts.length) % tv.alts.length };
    fitTo(tv.alts[tv.at].nodes);
  }
  const refOf = (i: number) => {
    const n = xs.nodes[i];
    return n.kind === "document" ? `doc:${n.ref}` : n.kind === "entity" ? `ent:${n.ref}` : n.kind === "chunk" ? `doc:${F.chunks[n.ref][0]}` : n.id;
  };
  function semanticPath(kind: "semantic" | "mixed") {
    if (tv?.kind !== "path") return;
    tv = { ...tv, semKind: kind, sem: semanticPaths(refOf(tv.from), refOf(tv.to), kind) };
    listTab = "view";
  }
  function setAllLinksAndRetry() {
    setMode("all");
    recomputePath();
  }

  function openImpact(root: number) {
    if (tv) closeView(true);
    tv = { kind: "impact", root, res: impactOf(refOf(root)) };
    selected = null;
    revision++;
    const ov = overlay.focusSet;
    if (ov) fitTo([...ov]);
    setTimeout(() => tv?.kind === "impact" && overlay.focusSet && fitTo([...overlay.focusSet]), 1200);
    announce(`Влияние изменения «${label(root)}»`);
  }
  function runContext() {
    if (!ctxForm) return;
    if (tv) closeView(true);
    const reply = contextFor(ctxForm.task);
    tv = { kind: "context", reply, task: ctxForm.task, profile: ctxForm.profile, scope: ctxForm.useSubgraph && subgraph.size ? `подграф: ${subgraphDocs().length} документов (передан областью документов)` : "текущая область" };
    ctxForm = null;
    selected = null;
    revision++;
  }
  function openTopics() {
    if (tv) closeView(true);
    popover = null;
    tv = { kind: "topics", open: new Set() };
    startRelayoutView();
    buildTopics();
    rebuild(0.9, true);
    announce("Обзор тем: темы свёрнуты, двойной щелчок раскрывает");
  }
  let topicsVisible: number[] = [];
  function buildTopics() {
    if (tv?.kind !== "topics") return;
    const t = buildLevel(xs, { level: "documents", expanded: {}, scenario, chunkLimit, showUnresolved: false, topics: tv.open });
    topicsVisible = t.visible;
    slice = toSlice(t);
  }
  function toggleTopic(topic: number) {
    if (tv?.kind !== "topics") return;
    const open = new Set(tv.open);
    if (open.has(topic)) open.delete(topic);
    else open.add(topic);
    tv = { ...tv, open };
    buildTopics();
    rebuild(0.6);
  }
  function openAudit() {
    if (tv) closeView(true);
    popover = null;
    tv = { kind: "audit" };
    loadLevel();
    rebuild(0.5);
    announce("Аудит связей: изоляты, неразрешённые ссылки и хабы");
  }

  // ---------- Подграф и область поиска
  function toggleSubgraph(index: number) {
    const next = new Set(subgraph);
    if (next.has(index)) next.delete(index);
    else next.add(index);
    subgraph = next;
  }
  /** Документы подграфа: чанк — свой документ, сущность — документы оснований, неразрешённая ссылка не даёт документа. */
  function subgraphDocs(): number[] {
    const docs = new Set<number>();
    for (const i of subgraph) {
      const n = xs.nodes[i];
      if (n.kind === "document") docs.add(n.ref);
      else if (n.kind === "chunk") docs.add(F.chunks[n.ref][0]);
      else if (n.kind === "entity") {
        for (const ev of F.entities[n.ref].evidence) {
          const e = evidenceOf(ev);
          if (e) docs.add(e.doc);
        }
      } else if (n.kind === "topic") for (const d of F.topics[n.ref].members) docs.add(d);
    }
    return [...docs];
  }
  function searchInSubgraph() {
    const docs = subgraphDocs();
    scopeUndo = app.selected;
    app.selected = new Set(docs.map((d) => F.docs[d][2]));
    notice = "";
    announce(`Область поиска: ${docs.length} документов из графа`);
    app.flash(`Область поиска: ${docs.length} документов из графа · «Вернуть прежнюю область» — в полосе подграфа`);
    (document.querySelector("textarea, input[type=search]") as HTMLElement | null)?.focus();
  }
  function undoScope() {
    if (!scopeUndo) return;
    app.selected = scopeUndo;
    scopeUndo = null;
    announce("Область поиска возвращена");
  }

  // ---------- Указатель
  function select(target: Target) {
    selected = target;
    if (target?.type === "node") notice = "";
    schedule();
  }
  function pointer(event: PointerEvent | MouseEvent): [number, number] {
    const r = canvas!.getBoundingClientRect();
    return [event.clientX - r.left, event.clientY - r.top];
  }
  function hit(sx: number, sy: number): Target {
    if (!layout) return null;
    let best: PNode | null = null;
    let bestD = Infinity;
    for (const n of layout.nodes) {
      const [x, y] = toScreen(cam, n.x, n.y);
      const d = Math.hypot(x - sx, y - sy);
      const reach = Math.max(8, radiusOf(n, cam) + 4);
      if (d < reach && d < bestD) {
        best = n;
        bestD = d;
      }
    }
    if (best) return { type: "node", index: best.index };
    let edge: GEdge | null = null;
    let edgeD = 6;
    for (const d of layoutLines(layout.links, kinds)) {
      const dist = lineDistance(d, cam, sx, sy);
      if (dist < edgeD) {
        edge = d.link.edge;
        edgeD = dist;
      }
    }
    return edge ? { type: "edge", id: edge.id } : null;
  }
  let drag: { mode: "pan" | "node"; index: number; sx: number; sy: number; cam: Camera; moved: boolean; target: Target; shift: boolean } | null = null;
  function onDown(event: PointerEvent) {
    if (event.button !== 0 || !canvas) return;
    canvas.setPointerCapture(event.pointerId);
    canvas.focus({ preventScroll: true });
    const [sx, sy] = pointer(event);
    const target = hit(sx, sy);
    drag = target?.type === "node" ? { mode: "node", index: target.index, sx, sy, cam, moved: false, target, shift: event.shiftKey } : { mode: "pan", index: -1, sx, sy, cam, moved: false, target, shift: event.shiftKey };
  }
  function onMove(event: PointerEvent) {
    if (!canvas) return;
    const [sx, sy] = pointer(event);
    if (drag) {
      if (Math.hypot(sx - drag.sx, sy - drag.sy) > 3) drag.moved = true;
      if (!drag.moved) return;
      if (drag.mode === "pan") {
        autoFitSince = 0;
        camTarget = null;
        cam = { ...drag.cam, x: drag.cam.x + sx - drag.sx, y: drag.cam.y + sy - drag.sy };
      } else {
        const [wx, wy] = toWorld(cam, sx, sy);
        layout?.dragTo(drag.index, wx, wy);
        if (!stopped && !reduced) layout?.reheat(0.25);
      }
      tip = null;
      schedule();
      return;
    }
    const target = hit(sx, sy);
    if (JSON.stringify(target) === JSON.stringify(hovered)) return;
    hovered = target;
    canvas.style.cursor = tv?.kind === "pick" ? "crosshair" : target ? "pointer" : "grab";
    if (target?.type === "node") tip = { text: `${label(target.index)} · ${KIND_NAME[nodeOf(target.index).kind]}${tv?.kind === "pick" ? " — щелчок: конец пути" : ""}`, x: sx, y: sy };
    else if (target?.type === "edge") tip = { text: `${edgeKindTitle(edgeOf(target.id))} · ${edgeTitle(edgeOf(target.id))}`, x: sx, y: sy };
    else tip = null;
    schedule();
  }
  function onUp() {
    if (!drag) return;
    const d = drag;
    drag = null;
    if (d.mode === "node") layout?.dragEnd(d.index);
    if (d.moved) return schedule();
    if (d.shift && d.target?.type === "node") toggleSubgraph(d.target.index);
    else if (tv?.kind === "pick" && d.target?.type === "node") finishPath(d.target.index);
    else select(d.target);
    schedule();
  }
  function onLeave() {
    if (drag) return;
    hovered = null;
    tip = null;
    schedule();
  }
  function onDouble(event: MouseEvent) {
    const [sx, sy] = pointer(event);
    const target = hit(sx, sy);
    if (target?.type !== "node") return;
    const n = nodeOf(target.index);
    if (tv?.kind === "topics" && n.kind === "topic") toggleTopic(n.ref);
    else toggleExpand(target.index);
  }
  function onWheel(event: WheelEvent) {
    event.preventDefault();
    const [sx, sy] = pointer(event);
    zoomAt(event.deltaY < 0 ? 1.12 : 1 / 1.12, sx, sy);
  }
  function neighbourInDirection(from: number, dx: number, dy: number): number | null {
    const a = layout?.get(from);
    if (!a || !layout) return null;
    let best: PNode | null = null;
    let bestScore = Infinity;
    for (const n of layout.nodes) {
      if (n.index === from) continue;
      const vx = n.x - a.x;
      const vy = n.y - a.y;
      const dist = Math.hypot(vx, vy) || 1;
      const cos = (vx * dx + vy * dy) / dist;
      if (cos < 0.5) continue;
      const score = dist * (2 - cos);
      if (score < bestScore) {
        best = n;
        bestScore = score;
      }
    }
    return best ? best.index : null;
  }

  /** Клавиши графа — по физической клавише (`code`), в любой раскладке; в полях ввода не действуют. */
  function onKey(event: KeyboardEvent) {
    const arrows: Record<string, [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };
    const sel = selected?.type === "node" ? selected.index : null;
    if (arrows[event.key]) {
      event.preventDefault();
      if (sel === null) {
        const first = [...(layout?.nodes ?? [])].sort((x, y) => y.degree - x.degree)[0];
        if (first) select({ type: "node", index: first.index });
        return;
      }
      const next = neighbourInDirection(sel, ...arrows[event.key]);
      if (next !== null) select({ type: "node", index: next });
    } else if (event.key === "Escape") {
      if (tv?.kind === "pick") {
        tv = null;
        announce("Выбор второй вершины отменён");
      } else select(null);
    } else if (event.key === "Enter" && sel !== null) {
      if (tv?.kind === "pick") finishPath(sel);
      else if (tv?.kind === "topics" && nodeOf(sel).kind === "topic") toggleTopic(nodeOf(sel).ref);
      else toggleExpand(sel);
    } else if (event.code === "Space" && sel !== null) {
      event.preventDefault();
      if (event.shiftKey) toggleSubgraph(sel);
      else pin(sel);
    } else if (event.code === "KeyM") cycleMode();
    else if (event.code === "KeyV") cycleLevel();
    else if (event.code === "KeyL" && sel !== null && !mini) openLocal(sel);
    else if (event.code === "KeyP" && sel !== null && !mini) startPath(sel);
    else if (event.code === "KeyI" && sel !== null && !mini) openImpact(sel);
    else if (event.code === "KeyO" && sel !== null) openSource(sel);
    else if (event.code === "BracketLeft") pathAlt(-1);
    else if (event.code === "BracketRight") pathAlt(1);
    else if (event.key === "Backspace") {
      if (tv) closeView();
      else if (query) leaveQuery();
    } else if (event.key === "+" || event.key === "=") zoomAt(1.25);
    else if (event.key === "-") zoomAt(0.8);
    else if (event.code === "KeyF" || event.key === "0") fit();
    else return;
    event.stopPropagation();
  }

  function pin(index: number) {
    const on = layout?.togglePin(index);
    notice = on ? "Узел закреплён: останется на месте при движении и перетаскивании других." : "Закрепление снято.";
    revision++;
    schedule();
  }

  /** Источник: документ, диапазон чанка, основание. Удалённый — не открывается; изменённый — открывается целиком с предупреждением. */
  function openSource(index: number, line?: number) {
    const n = nodeOf(index);
    if (n.deleted) {
      notice = "Источник удалён после снимка: открыть нечего, цитата снимка сохранена в инспекторе.";
      return;
    }
    if (n.kind === "entity") {
      const ev = F.entities[n.ref].evidence[0];
      if (ev) openEvidence(ev);
      return;
    }
    if (n.kind === "ghost" || n.kind === "topic" || n.kind === "query") return;
    const stale = n.kind === "document" ? n.stale : nodeOf(n.owner)?.stale;
    const first = stale ? 0 : (line ?? (n.kind === "chunk" ? n.firstLine : 0));
    const last = stale ? 0 : (line ?? (n.kind === "chunk" ? n.lastLine : 0));
    if (stale) notice = "Изменён после снимка: открыт документ целиком, строки снимка могли сдвинуться.";
    void app.openDocument({ path: n.path, title: n.kind === "chunk" ? label(n.owner) : n.label, first_line: first, last_line: last });
  }
  function openEvidence(id: string) {
    const ev = evidenceOf(id);
    if (!ev) return;
    const docIndex = xs.indexOf(`doc:${ev.doc}`);
    const doc = docIndex !== undefined ? nodeOf(docIndex) : null;
    if (doc?.deleted) {
      notice = "Источник основания удалён после снимка: цитата снимка сохранена.";
      return;
    }
    if (doc?.stale) notice = "Документ основания изменён после снимка: открыт целиком, строки могли сдвинуться.";
    void app.openDocument({ path: ev.path, title: ev.title, first_line: doc?.stale ? 0 : ev.first, last_line: doc?.stale ? 0 : ev.last });
  }
  /** «Показать в графе» из инспектора запроса: выбор, центрирование и короткая подсветка (без пульса при reduced motion). */
  function showResult(index: number) {
    select({ type: "node", index });
    const p = layout?.get(index);
    if (p && size.w > 1) {
      const k = Math.max(cam.k, 1.1);
      const target = { k, x: size.w / 2 - p.x * k, y: size.h / 2 - p.y * k };
      if (reduced) cam = target;
      else camTarget = target;
    }
    pulseUntil = reduced ? 0 : performance.now() + 1600;
    schedule();
  }

  const runFind = (event: KeyboardEvent) => {
    if (event.key !== "Enter") return;
    const q = findText.trim().toLowerCase();
    const found = layout?.nodes.filter((n) => n.node.label.toLowerCase().includes(q)) ?? [];
    if (!found.length) return;
    if (tv?.kind === "pick") finishPath(found[0].index);
    else select({ type: "node", index: found[0].index });
  };

  // ---------- Данные разметки
  const KIND_NAME: Record<string, string> = { document: "документ", chunk: "чанк", query: "запрос", entity: "сущность", ghost: "неразрешённая ссылка", topic: "тема" };
  const KIND_HEAD: Record<string, string> = { document: "Документ", chunk: "Чанк", query: "Запрос", entity: "Сущность", ghost: "Неразрешённая ссылка", topic: "Тема" };
  const selectedNode = $derived(selected?.type === "node" ? nodeOf(selected.index) : null);
  const selectedEdge = $derived(selected?.type === "edge" ? edgeOf(selected.id) : null);
  const connections = $derived.by(() => {
    void revision;
    const sel = selected;
    if (sel?.type !== "node" || !layout) return [] as { edge: XEdge; other: number }[];
    return layout.links
      .filter((l) => l.source.index === sel.index || l.target.index === sel.index)
      .map((l) => ({ edge: l.edge as unknown as XEdge, other: l.source.index === sel.index ? l.target.index : l.source.index }))
      .sort((a, b) => b.edge.s - a.edge.s);
  });
  const pinnedSel = $derived.by(() => {
    void revision;
    return selectedNode ? (layout?.get(selectedNode.index)?.pinned ?? false) : false;
  });
  const nodeCount = $derived.by(() => {
    void revision;
    return layout?.nodes.length ?? 0;
  });
  const linkCount = $derived.by(() => {
    void revision;
    return layout?.links.length ?? 0;
  });
  const rows = $derived.by(() => {
    void revision;
    const q = listFilter.trim().toLowerCase();
    return (layout?.nodes ?? [])
      .filter((n) => !q || n.node.label.toLowerCase().includes(q) || (n.node.path ?? "").toLowerCase().includes(q))
      .sort((a, b) => b.degree - a.degree || a.node.label.localeCompare(b.node.label));
  });
  const edgeRows = $derived.by(() => {
    void revision;
    const q = listFilter.trim().toLowerCase();
    return (layout?.links ?? []).map((l) => l.edge as unknown as XEdge).filter((e) => !q || edgeTitle(e).toLowerCase().includes(q));
  });
  const MOTION_TEXT = { running: "Движение идёт", idle: "Раскладка в покое", stopped: "Остановлено вами", reduced: "Без движения (reduced motion)" };
  const dash = (kind: XKind) => (kind === "chunk_similarity" ? "5 3" : kind === "mention" ? "1 3" : kind === "unresolved" ? "3 3" : "0");
  const reverseLink = (e: XEdge) => (e.kind === "markdown_link" ? (slice.edges as unknown as XEdge[]).find((x) => x.kind === "markdown_link" && x.a === e.b && x.b === e.a) : undefined);
  function edgeTitle(e: XEdge): string {
    if (e.kind === "assertion" && e.assertion !== undefined) {
      const a = F.assertions[e.assertion];
      return `${label(e.a)} ${a.polarity === "negative" ? "не " : ""}${PREDICATE_NAME[a.predicate] ?? a.predicate} ${label(e.b)}`;
    }
    if (e.kind === "mention") return `«${label(e.a)}» упоминает «${label(e.b)}»`;
    if (e.kind === "unresolved") return `${label(e.a)} → ${label(e.b)} (нет цели)`;
    if (e.kind !== "markdown_link") return `${label(e.a)} — ${label(e.b)}`;
    return reverseLink(e) ? `${label(e.a)} и ${label(e.b)} ссылаются друг на друга` : `${label(e.a)} → ${label(e.b)}`;
  }
  const edgeKindTitle = (e: XEdge) => (e.kind === "assertion" && e.assertion !== undefined ? `Утверждение · ${F.assertions[e.assertion].predicate}` : XKIND_TITLE[e.kind]);
  const linesText = (e: XEdge) => (e.lines.length ? `строки ${e.lines.slice(0, 8).join(", ")}${e.lines.length > 8 ? "…" : ""}` : "строки неизвестны");
  const CONN_KINDS: XKind[] = ["markdown_link", "document_similarity", "chunk_similarity", "query_result", "assertion", "mention", "unresolved", "aggregate"];
  const ORIGIN: Record<string, string> = { explicit_rule: "явная запись (правило извлечения)", model: "модельный кандидат", structural: "структура Markdown", similarity: "близость векторов", manual: "ручная проверка" };
  /** Виды связей режима на текущем уровне: на уровне чанков «по смыслу» — близость чанков. */
  const levelModeKinds = (m: Mode): string[] => (lvlOf() === "chunks" ? (m === "links" ? ["markdown_link"] : m === "meaning" ? ["chunk_similarity"] : ["markdown_link", "chunk_similarity"]) : modeKindsList(m));
  const modeCount = (m: Mode) => (lvl.edges as XEdge[]).filter((e) => levelModeKinds(m).includes(e.kind) && passes(e)).length;
  const noNodes = $derived(loaded && !loadError && lvl.visible.length === 0 && tv?.kind !== "topics");
  const subgraphDocCount = $derived.by(() => {
    void subgraph;
    return subgraphDocs().length;
  });
  const entityAssertions = (ent: number) => F.assertions.map((a, k) => ({ a, k })).filter(({ a }) => a.from === ent || a.to === ent);
  const entityMentions = (ent: number) => F.mentions.filter(([, e]) => e === ent);
  const assertionGroups = (ent: number) => {
    const own = entityAssertions(ent);
    return [{ title: "Исходящие", list: own.filter((x) => x.a.from === ent) }, { title: "Входящие", list: own.filter((x) => x.a.to === ent) }];
  };
  function selectAssertion(k: number) {
    const e = (slice.edges as unknown as XEdge[]).find((y) => y.assertion === k);
    if (e) select({ type: "edge", id: e.id });
  }
  const fileErrors = scenario === "fileerrors" ? SCENARIO_TARGETS.fileErrors : [];
  const viewNodeCount = $derived(overlay.focusSet?.size ?? 0);
  const localAtMaximum = $derived(tv?.kind === "local" && tv.budget >= 500);

  // ---------- Жизненный цикл
  onMount(() => {
    mini = !!host.closest(".mini");
    palette = readPalette(host);
    mounted = true;
    general = new Layout();
    general.onPositions = onPositions;
    general.setForces($state.snapshot(settings.forces) as Forces);
    layout = general;
    loadLevel();
    rebuild(0.9, true);
    hostObserver = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(([entry]) => {
      const w = Math.floor(entry.contentRect.width);
      const h = Math.floor(entry.contentRect.height);
      if (fitted) cam = { ...cam, x: cam.x + (w - size.w) / 2, y: cam.y + (h - size.h) / 2 };
      size = { w, h };
      if (canvas) {
        const dpr = window.devicePixelRatio || 1;
        canvas.width = Math.max(1, Math.round(w * dpr));
        canvas.height = Math.max(1, Math.round(h * dpr));
      }
      if (!fitted && w > 1 && h > 1 && layout?.nodes.length) fit(true);
      schedule();
    });
    hostObserver?.observe(stage);
    themeObserver = new MutationObserver(() => {
      palette = readPalette(host);
      schedule();
    });
    themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["data-mode"] });
    // Параметры адреса: выбранный узел и временный вид для снимков.
    if (!mini) setTimeout(applyUrl, Number(P.get("delay")) || 2500);
    const api = {
      layout: () => layout,
      cam: () => cam,
      selected: () => selected,
      level: () => level,
      mode: () => mode,
      view: () => tv?.kind ?? null,
      tv: () => tv,
      overlay: () => overlay,
      subgraph: () => [...subgraph],
      nodeCount: () => nodeCount,
      linkCount: () => linkCount,
      find: (part: string) => xs.nodes.findIndex((n) => n && lvl.visible.includes(n.index) && n.label.toLowerCase().includes(part.toLowerCase())),
      select: (index: number) => select({ type: "node", index }),
      general: () => general,
      scope: () => [...app.selected],
    };
    const w = window as unknown as { __graphActions?: Record<string, unknown> };
    w.__graphActions = { ...(w.__graphActions ?? {}), [mini ? "mini" : "full"]: api };
  });
  function applyUrl() {
    const want = P.get("select");
    const idx = want ? xs.nodes.findIndex((n) => n && lvl.visible.includes(n.index) && n.label.toLowerCase().includes(want.toLowerCase())) : -1;
    if (idx >= 0) select({ type: "node", index: idx });
    const v = P.get("view");
    if (v === "topics") openTopics();
    else if (v === "audit") openAudit();
    else if (v === "context") ctxForm = { task: F.context[0].query, profile: "implementation", budget: 8000, useSubgraph: false };
    else if (idx >= 0 && v === "local") openLocal(idx);
    else if (idx >= 0 && v === "impact") openImpact(idx);
    else if (idx >= 0 && v === "path") {
      startPath(idx);
      const to = P.get("to");
      const j = to ? xs.nodes.findIndex((n) => n && lvl.visible.includes(n.index) && n.label.toLowerCase().includes(to.toLowerCase())) : -1;
      if (j >= 0) finishPath(j);
      if (P.get("sem")) semanticPath(P.get("sem") === "mixed" ? "mixed" : "semantic");
    }
    if (P.get("sub")) for (const part of P.get("sub")!.split(",")) {
      const j = xs.nodes.findIndex((n) => n && lvl.visible.includes(n.index) && n.label.toLowerCase().includes(part.toLowerCase()));
      if (j >= 0) toggleSubgraph(j);
    }
    if (P.get("list")) {
      view = "list";
      listTab = P.get("list") as typeof listTab;
    }
    if (P.get("pop")) popover = P.get("pop") as typeof popover;
    if (P.get("ctx") && v === "context") runContext();
  }
  onDestroy(() => {
    if (raf) cancelAnimationFrame(raf);
    hostObserver?.disconnect();
    themeObserver?.disconnect();
    if (layout && layout !== general) layout.destroy();
    general?.destroy();
    layout = null;
  });
  $effect(() => {
    void selected;
    void findText;
    void subgraph;
    void tv;
    schedule();
  });
</script>

{#snippet forcesForm()}
  {#if mode !== "links" && lvlOf() !== "entities"}
    <h4>Связи по смыслу</h4>
    <label class="opt thr-row"><span>Сила от</span><input type="range" min="0.5" max="0.95" step="0.01" aria-label="Порог показа близости" bind:value={settings.minS} oninput={() => rebuild(0.35)} /><b>{minS.toFixed(2)}</b></label>
  {/if}
  <h4>Раскладка</h4>
  <label class="opt slider"><span>Притяжение связей</span><input type="range" min="0" max="4" step="0.1" aria-label="Притяжение по связям" bind:value={settings.forces.link} oninput={() => layout?.setForces($state.snapshot(forces) as Forces)} /><b>×{forces.link.toFixed(1)}</b></label>
  <label class="opt slider"><span>Отталкивание</span><input type="range" min="0" max="4" step="0.1" aria-label="Отталкивание узлов" bind:value={settings.forces.repel} oninput={() => layout?.setForces($state.snapshot(forces) as Forces)} /><b>×{forces.repel.toFixed(1)}</b></label>
  <p class="hint">Остальные настройки раскладки — как в продуктовой панели (2.1.6).</p>
{/snippet}

<div class="pgw" bind:this={host}>
<div class="pg" class:mini role="region" aria-label={mini ? "Мини-граф" : "Граф проекта"} data-level={lvlOf()} data-mode={mode} data-view={tv?.kind ?? ""} data-nodes={nodeCount} data-links={linkCount} data-loaded={loaded}>
  {#if !mini}
    <div class="tops">
      <div class="bar" role="toolbar" aria-label="Управление графом">
        <label class="find">
          <Icon name="search" size={13} width={1.8} />
          <input type="search" placeholder={tv?.kind === "pick" ? "Конец пути: имя узла, Enter" : "Найти узел"} aria-label="Найти узел графа" bind:value={findText} onkeydown={runFind} />
          {#if findText.trim()}<span class="found" role="status">{markedNodes().size}</span>{/if}
        </label>
        <div class="seg" role="radiogroup" aria-label="Уровень карты">
          {#each LEVELS as l (l)}
            <button class="mode" role="radio" aria-checked={level === l} title="{LEVEL_NAME[l]}. Клавиша V — следующий уровень" onclick={() => setLevel(l)}>
              <span class="shape {l}" aria-hidden="true"></span>{LEVEL_NAME[l]}{#if l === "entities" || l === "mixed"}<span class="state partial" title="Слой сущностей экспериментальный">эксп.</span>{/if}
            </button>
          {/each}
        </div>
        {#if lvlOf() === "entities"}
          <button class="mode solo" aria-expanded={popover === "predicates"} onclick={() => (popover = popover === "predicates" ? null : "predicates")} title="На уровне сущностей связи — утверждения; фильтр предикатов вместо режима связей">Предикаты {filters.predicates.size}/{ALL_PREDICATES.length} ▾</button>
        {:else}
          <div class="seg" role="radiogroup" aria-label="Режим связей">
            {#each availableModes as m (m)}
              <button class="mode" role="radio" aria-checked={mode === m} title="{MODE_NAME[m]}. Клавиша M — следующий режим" onclick={() => setMode(m)}>{MODE_NAME[m]} <b>{modeCount(m)}</b></button>
            {/each}
          </div>
        {/if}
        <span class="sep"></span>
        <div class="tools">
          <button aria-label="Уменьшить масштаб" title="Уменьшить масштаб" onclick={() => zoomAt(0.8)}><Icon name="minus" size={14} width={1.8} /></button>
          <button aria-label="Увеличить масштаб" title="Увеличить масштаб" onclick={() => zoomAt(1.25)}><Icon name="plus" size={14} width={1.8} /></button>
          <button aria-label="Вписать" title="Вписать граф в панель (F)" onclick={() => fit()}><Icon name="fit" size={14} width={1.8} /></button>
          <button aria-label={stopped ? "Запустить движение" : "Остановить движение"} aria-pressed={stopped} title={stopped ? "Запустить движение" : "Остановить движение"} onclick={() => setStopped(!stopped)} disabled={reduced}><Icon name={stopped ? "play" : "pause"} size={14} width={1.8} /></button>
          <button aria-label={view === "list" ? "Показать карту" : "Показать списком"} aria-pressed={view === "list"} title={view === "list" ? "Показать карту" : "Показать списком"} onclick={() => (view = view === "list" ? "map" : "list")} disabled={!!rendererError}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01" /></svg>
          </button>
          <button class="badged" aria-label="Фильтры карты{activeFilters ? `: активно ${activeFilters}` : ''}" aria-expanded={popover === "filters"} title="Фильтры" onclick={() => (popover = popover === "filters" ? null : "filters")}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round" aria-hidden="true"><path d="M3 5h18l-7 8v6l-4 2v-8z" /></svg>{#if activeFilters}<i>{activeFilters}</i>{/if}
          </button>
          <button aria-label="Настройки раскладки" aria-expanded={popover === "settings"} title="Настройки раскладки" onclick={() => (popover = popover === "settings" ? null : "settings")}><Icon name="gear" size={14} width={1.8} /></button>
          <button aria-label="Легенда" aria-expanded={legendOpen} title="Легенда" onclick={() => (legendOpen = !legendOpen)}><Icon name="info" size={14} width={1.8} /></button>
        </div>
        <button class="menu-btn" aria-expanded={popover === "analysis"} onclick={() => (popover = popover === "analysis" ? null : "analysis")}>Анализ ▾</button>
      </div>

      {#if tv}
        <div class="vbar" role="region" aria-label="Временный вид: {VIEW_NAME[tv.kind]}">
          <span class="vic" aria-hidden="true"></span>
          {#if tv.kind === "local"}
            <b class="vt" title={label(tv.center)}>Локальный граф «{label(tv.center)}»</b>
            <span class="depth" role="radiogroup" aria-label="Глубина локального графа">Глубина
              {#each [1, 2, 3] as d (d)}<button role="radio" aria-checked={tv.depth === d} class:on={tv.depth === d} disabled={tv.res.limited && d > tv.depth} title={tv.res.limited && d > tv.depth ? "Достигнут предел обхода: сначала «Показать ещё»" : `Глубина ${d}`} onclick={() => setLocalDepth(d)}>{d}</button>{/each}
            </span>
            <span>связи: {lvlOf() === "entities" ? "утверждения" : MODE_NAME[mode].toLowerCase()} · узлов {tv.res.depthOf.size}</span>
            <button class="more ghost" onclick={selectLocalAsSubgraph}>Выбрать весь локальный граф</button>
          {:else if tv.kind === "pick"}
            <b class="vt">Путь от «{label(tv.from)}»: выберите вторую вершину щелчком, в поиске узла или в списке</b>
            <button class="more" onclick={() => { tv = null; announce("Выбор второй вершины отменён"); }}>Отмена</button>
          {:else if tv.kind === "path"}
            {@const p = tv.alts[tv.at]}
            <b class="vt">Путь по карте: «{label(tv.from)}» → «{label(tv.to)}»</b>
            {#if p}
              <span>{p.steps.length} шаг{p.steps.length === 1 ? "" : p.steps.length < 5 ? "а" : "ов"} · вариант {tv.at + 1} из {tv.alts.length}</span>
              <span class="alt"><button aria-label="Предыдущий вариант ([)" onclick={() => pathAlt(-1)} disabled={tv.alts.length < 2}>◀</button><button aria-label="Следующий вариант (])" onclick={() => pathAlt(1)} disabled={tv.alts.length < 2}>▶</button></span>
            {:else}
              <span class="warnt">В режиме «{MODE_NAME[mode]}» пути нет</span>
              {#if mode !== "all" && lvlOf() !== "entities"}<button class="more ghost" onclick={setAllLinksAndRetry}>Искать во всех связях</button>{/if}
            {/if}
            <label class="chk"><input type="checkbox" checked={tv.directed} onchange={(e) => { if (tv?.kind === "path") { tv = { ...tv, directed: e.currentTarget.checked }; recomputePath(); } }} /> по направлению ссылок</label>
            <button class="more ghost" onclick={() => semanticPath("semantic")}>Смысловой путь</button>
          {:else if tv.kind === "impact"}
            <b class="vt" title={label(tv.root)}>Влияние изменения «{label(tv.root)}»</b>
            <span><i class="ring direct"></i>прямое {tv.res.direct.length} · <i class="ring indirect"></i>косвенное {tv.res.indirect.length} · <i class="ring neighbour"></i>ссылки-соседи {tv.res.neighbours.length}</span>
          {:else if tv.kind === "context"}
            <b class="vt" title={tv.task}>Контекст «{tv.task}»</b>
            <span>{tv.profile === "implementation" ? "реализация" : tv.profile === "investigation" ? "исследование" : "решение"} · {6 - tv.reply.missing.length} из 6 ролей · {(tv.reply.used / 1024).toFixed(1)} КБ, бюджет {tv.reply.mode === "estimated" ? "оценочный" : "точный"}</span>
          {:else if tv.kind === "topics"}
            <b class="vt">Обзор тем · {F.topics.length} тем, сообщества</b>
            <span>раскрыто {tv.open.size}; двойной щелчок или Enter раскрывает тему</span>
          {:else if tv.kind === "audit"}
            <b class="vt">Аудит связей · структурный слой</b>
            <span><i class="ring isolate"></i>изоляты {F.audit.filter((a) => a.kind === "structural_isolate").length} · неразрешённые ссылки {F.broken.length} · <i class="ring hub"></i>хабы {hubs().length}</span>
          {/if}
          <button class="more" onclick={() => closeView()}>Общий вид</button>
        </div>
        {#if tv.kind === "path"}<div class="banner subtle" role="note">Путь по карте показывает соседство на карте, а не зависимость. Основания каждого шага — в инспекторе.</div>{/if}
        {#if tv.kind === "local" && tv.res.truncated}
          <div class="banner" role="status"><span><b>Достигнут предел обхода:</b> показано {tv.res.depthOf.size} из {tv.res.known} узлов; следующий шаг глубины недоступен.</span>{#if localAtMaximum}<span>Максимум вида: уменьшите глубину или режим связей.</span>{:else}<button class="more" onclick={moreLocal}>Показать ещё</button>{/if}</div>
        {/if}
      {/if}

      {#if query}
        <div class="qbar" role="region" aria-label="Вид запроса">
          <span class="qsq" aria-hidden="true"></span>
          <b class="qtext" title={query.text}>Запрос «{query.text}»</b>
          <span>{query.results.length} результатов</span>
          <span>{SEARCH_MODE[query.applied]} · {SCORE_TEXT[query.scoreKind]}</span>
          <span class="depth" role="radiogroup" aria-label="Окружение результатов">Окружение
            {#each Array.from({ length: MAX_DEPTH + 1 }, (_, i) => i) as d (d)}<button role="radio" aria-checked={queryDepth === d} class:on={queryDepth === d} onclick={() => { queryDepth = d; queryBudget = { ...QUERY_BUDGET }; rebuild(0.5); }}>{d}</button>{/each}
          </span>
          <button class="more" onclick={leaveQuery}>Общий вид</button>
        </div>
      {/if}

      {#if subgraph.size}
        <div class="sbar" role="region" aria-label="Подграф">
          <span class="sq" aria-hidden="true"></span>
          <b>Подграф: {subgraph.size} узл{subgraph.size === 1 ? "" : subgraph.size < 5 ? "а" : "ов"} · {subgraphDocCount} документ{subgraphDocCount === 1 ? "" : subgraphDocCount < 5 ? "а" : "ов"}</b>
          <button class="more" onclick={searchInSubgraph} disabled={!subgraphDocCount}>Искать в подграфе</button>
          <button class="more ghost" onclick={() => (ctxForm = { task: "", profile: "implementation", budget: 8000, useSubgraph: true })}>Собрать контекст</button>
          {#if scopeUndo}<button class="more ghost" onclick={undoScope}>Вернуть прежнюю область</button>{/if}
          <button class="more ghost" onclick={() => (subgraph = new Set())}>Очистить</button>
        </div>
      {:else if scopeUndo}
        <div class="sbar" role="status"><span>Область поиска заменена подграфом.</span><button class="more ghost" onclick={undoScope}>Вернуть прежнюю область</button></div>
      {/if}

      {#each lvl.banner as text, i (i)}
        {#if lvlOf() === "entities" || lvlOf() === "mixed" || text.startsWith("Не обработано")}<div class="banner" class:warn={scenario === "lagging" || text.startsWith("Не обработано")} role="status"><span>{text}</span></div>{/if}
      {/each}
      {#if hiddenByFilters && (hiddenByFilters.nodes || hiddenByFilters.edges)}
        <div class="banner" role="status"><span><b>Фильтры скрывают</b> {hiddenByFilters.nodes} узлов и {hiddenByFilters.edges} связей.</span><button class="more" onclick={() => { filters = freshFilters(); loadLevel(); rebuild(0.5); }}>Сбросить</button></div>
      {/if}
      {#if scenario === "stale" || scenario === "deleted"}
        <div class="banner warn" role="status"><span>{scenario === "stale" ? `${SCENARIO_TARGETS.stale.length} документа изменены после снимка: их узлы бледнее и с часами.` : "Источник удалён после снимка: узел пунктирный и перечёркнут."}</span><button class="more" onclick={() => announce("Стенд: обновление среза запросило бы новый снимок у воркера")}>Обновить срез</button></div>
      {/if}
      {#if lvl.truncated && !tv}
        <div class="banner" role="status"><span><b>Граф усечён:</b> {lvl.limitReasons.join("; ")}. Связи между показанными узлами полны.</span>{#if chunkLimit < CHUNK_MAX}<button class="more" onclick={() => { chunkLimit = Math.min(CHUNK_MAX, chunkLimit * 2); loadLevel(); rebuild(0.4); }}>Показать ещё</button>{/if}</div>
      {/if}
    </div>
  {/if}

  <div class="stage" bind:this={stage}>
    {#if view === "map"}
      {#if loadError}
        <div class="empty" role="alert"><b>Граф ещё не построен или устарел</b><span>{loadError}. Поиск и дерево работают; уровни сущностей ждут смыслового слоя.</span><button class="act" onclick={() => announce("Стенд: задание построения графа было бы принято воркером")}>Обновить граф</button></div>
      {:else if noNodes}
        <div class="empty" role="status"><b>В выбранной области нет документов</b><span>Измените область в дереве или в фильтре документов.</span></div>
      {/if}
      <canvas
        bind:this={canvas}
        class:hide={!loaded || !!loadError || noNodes}
        tabindex="0"
        aria-label="Карта графа. Стрелки — соседний узел, V — уровень, M — режим, L — локальный граф, P — путь, I — влияние, O — открыть, Shift+пробел — подграф, Backspace — общий вид"
        onpointerdown={onDown}
        onpointermove={onMove}
        onpointerup={onUp}
        onpointercancel={onUp}
        onpointerleave={onLeave}
        ondblclick={onDouble}
        onwheel={onWheel}
        onkeydown={onKey}
      ></canvas>
      {#if tip}<div class="tip" style:left="{Math.min(tip.x + 12, size.w - 190)}px" style:top="{Math.max(2, tip.y + 14)}px" role="tooltip">{tip.text}</div>{/if}
      {#if loaded && !loadError && scenario === "nolinks"}<p class="note" role="status">У документов области нет допущенных связей: порог близости не пройден, ссылок нет.</p>{/if}
      {#if toast}<div class="toast" role="status" aria-live="polite">{toast}</div>{/if}
      <div class="status" role="status">
        <span class="dot {motion}"></span>
        {#if mini}<b>Документы {slice.docCount}</b> · {MODE_NAME[mode].toLowerCase()}
        {:else}{MOTION_TEXT[motion]} · <b>{LEVEL_NAME[lvlOf()]}</b> · узлов {nodeCount}{#if lvl.totals.nodes > nodeCount && !tv} из {lvl.totals.nodes}{/if} · связей {linkCount}{#if tv && viewNodeCount} · в виде {viewNodeCount}{/if}{/if}
      </div>
      {#if queryOffered}
        <div class="qplate dashed"><span class="qt">Запрос «{standQuery}»</span><button class="inline" onclick={enterQuery}>{mini ? "Показать" : "Показать запрос в графе"}</button></div>
      {/if}
      {#if mini && selectedNode}
        <div class="caption" role="status">
          <b title={selectedNode.label}>{selectedNode.label}</b>
          <button class="inline" onclick={() => openSource(selectedNode.index)}>Открыть</button>
          <button class="inline" onclick={() => (app.view = "graph")}>В графе</button>
        </div>
      {/if}
      {#if popover === "settings"}<section class="pop" class:full={!mini} aria-label="Настройки раскладки">{@render forcesForm()}</section>{/if}
      {#if popover === "analysis" && !mini}
        <section class="pop full menu" aria-label="Анализ">
          <button onclick={() => { popover = null; ctxForm = { task: "", profile: "implementation", budget: 8000, useSubgraph: subgraph.size > 0 }; }}><b>Контекст задачи…</b><small>источники по ролям с бюджетом (S3)</small></button>
          <button onclick={openTopics}><b>Обзор тем</b><small>темы — свёрнутые группы, связи между ними (S6)</small></button>
          <button onclick={openAudit}><b>Аудит связей</b><small>изоляты, неразрешённые ссылки, хабы (S8)</small></button>
          <p class="hint">Связи, путь и влияние — действия выбранного узла в инспекторе.</p>
        </section>
      {/if}
      {#if popover === "predicates" && !mini}
        <section class="pop full" aria-label="Фильтр предикатов">
          {#each Object.entries(GROUP_NAME) as [group, name] (group)}
            {@const preds = ALL_PREDICATES.filter((p) => PREDICATE_GROUP[p] === group)}
            {#if preds.length}
              <h4>{name}</h4>
              {#each preds as p (p)}<label class="chk"><input type="checkbox" checked={filters.predicates.has(p)} onchange={(e) => { const s = new Set(filters.predicates); if (e.currentTarget.checked) s.add(p); else s.delete(p); filters.predicates = s; rebuild(0.5); }} /> {p} · {PREDICATE_NAME[p]} <small>{F.assertions.filter((a) => a.predicate === p).length}</small></label>{/each}
            {/if}
          {/each}
          <p class="hint">Близость и совместная встречаемость на уровнях сущностей выключены и включаются отдельно в фильтрах.</p>
        </section>
      {/if}
      {#if popover === "filters" && !mini}
        <section class="pop full filters" aria-label="Фильтры карты">
          <h4>Область</h4>
          <p class="hint">Текущая область поиска: {app.selected.size ? `${app.selected.size} отмечено в дереве` : "весь проект"}. <button class="inline" onclick={() => announce("Область меняется галочками дерева")}>Изменить в дереве</button>. Камера область не меняет.</p>
          <h4>Типы узлов</h4>
          <label class="chk"><input type="checkbox" bind:checked={filters.nodeKinds.document} onchange={() => rebuild(0.5)} /> документы</label>
          <label class="chk"><input type="checkbox" bind:checked={filters.nodeKinds.chunk} onchange={() => rebuild(0.5)} /> чанки</label>
          <label class="chk"><input type="checkbox" bind:checked={filters.nodeKinds.entity} onchange={() => rebuild(0.5)} /> сущности</label>
          <h4>Направление (от выбранного или центра)</h4>
          <div class="radios">{#each [["both", "обе"], ["out", "исходящие"], ["in", "входящие"]] as [v, t] (v)}<label class="chk"><input type="radio" name="dir" checked={filters.direction === v} onchange={() => { filters.direction = v as Filters["direction"]; rebuild(0.4); }} /> {t}</label>{/each}</div>
          <h4>Утверждения</h4>
          <label class="chk"><input type="checkbox" bind:checked={filters.explicit} onchange={() => rebuild(0.4)} /> явная запись</label>
          <label class="chk"><input type="checkbox" bind:checked={filters.candidate} onchange={() => rebuild(0.4)} /> модельный кандидат</label>
          <div class="radios">{#each [["all", "все"], ["with", "с условиями"], ["without", "без условий"]] as [v, t] (v)}<label class="chk"><input type="radio" name="cond" checked={filters.conditions === v} onchange={() => { filters.conditions = v as Filters["conditions"]; rebuild(0.4); }} /> {t}</label>{/each}</div>
          <label class="chk"><input type="checkbox" bind:checked={filters.negations} onchange={() => rebuild(0.4)} /> показывать отрицания</label>
          <h4>Ссылки</h4>
          <label class="chk"><input type="checkbox" bind:checked={filters.unresolved} onchange={() => { loadLevel(); rebuild(0.5); }} /> неразрешённые ссылки призрачными узлами ({F.broken.length})</label>
          <p class="hint">Режим связей Т-46 остаётся единственным переключателем ссылок и близости; фильтры его не дублируют.</p>
          <button class="reset" onclick={() => { filters = freshFilters(); loadLevel(); rebuild(0.5); }}>Сбросить фильтры</button>
        </section>
      {/if}
      {#if legendOpen && !mini}
        <section class="legend" aria-label="Легенда графа">
          <h4>Узлы — форма, не цвет</h4>
          <div class="row"><span class="shape documents"></span>документ <span class="shape chunks"></span>чанк <span class="shape entities"></span>сущность</div>
          <div class="row"><span class="shape ghost"></span>неразрешённая ссылка <span class="shape topic"></span>тема</div>
          <h4>Связи</h4>
          <div class="row"><svg width="34" height="10"><line x1="1" y1="5" x2="28" y2="5" stroke="currentColor" /><path d="M33 5l-5-3v6z" /></svg>ссылка, утверждение — стрелка к цели</div>
          <div class="row"><svg width="34" height="10"><line x1="1" y1="5" x2="33" y2="5" stroke="currentColor" stroke-dasharray="5 3" /></svg>близость чанков</div>
          <div class="row"><svg width="34" height="10"><line x1="1" y1="5" x2="33" y2="5" stroke="currentColor" stroke-dasharray="1 3" /></svg>упоминание</div>
          <div class="row"><svg width="34" height="10"><line x1="1" y1="5" x2="33" y2="5" stroke="currentColor" stroke-dasharray="6 2 1 2" /></svg>отрицание «не»</div>
          <div class="row"><svg width="34" height="10"><line x1="1" y1="5" x2="33" y2="5" stroke="currentColor" stroke-dasharray="2 3" /></svg>модельный кандидат</div>
          <h4>Отметки</h4>
          <p class="hint">«если» — условие; «!» — кандидат конфликта; часы — изменён после снимка; перечёркнутый пунктир — источник удалён; двойной контур «?» — неоднозначная сущность; номер — шаг пути; буква — роль контекста; квадрат-галочка — подграф.</p>
        </section>
      {/if}
    {:else}
      <div class="list">
        <div class="list-tools">
          <div class="tabs" role="tablist" aria-label="Список графа">
            <button role="tab" aria-selected={listTab === "nodes"} onclick={() => (listTab = "nodes")}>Узлы {rows.length}</button>
            <button role="tab" aria-selected={listTab === "edges"} onclick={() => (listTab = "edges")}>Связи {edgeRows.length}</button>
            <button role="tab" aria-selected={listTab === "view"} disabled={!tv} onclick={() => (listTab = "view")}>Вид{tv ? `: ${VIEW_NAME[tv.kind]}` : ""}</button>
          </div>
          <input type="search" placeholder="Фильтр списка" aria-label="Фильтр списка" bind:value={listFilter} />
        </div>
        {#if rendererError}<p class="note" role="alert">{rendererError}</p>{/if}
        {#if listTab === "nodes"}
          <ul aria-label="Узлы графа">
            {#each rows.slice(0, 400) as n (n.index)}
              {@const xn = n.node as unknown as XNode}
              <li>
                <input type="checkbox" aria-label="В подграф: {n.node.label}" checked={subgraph.has(n.index)} onchange={() => toggleSubgraph(n.index)} />
                <button class:on={selected?.type === "node" && selected.index === n.index} onclick={() => (tv?.kind === "pick" ? finishPath(n.index) : select({ type: "node", index: n.index }))}>
                  <span class="shape {xn.kind === 'document' ? 'documents' : xn.kind === 'chunk' ? 'chunks' : xn.kind === 'entity' ? 'entities' : xn.kind}"></span><span class="name" title={n.node.label}>{n.node.label}</span>{#if overlay.stepOf.has(n.index)}<em>шаг {overlay.stepOf.get(n.index)}</em>{/if}{#if xn.stale}<em>изменён</em>{/if}{#if xn.deleted}<em>удалён</em>{/if}<small>{n.node.module}</small><b>{n.degree}</b>
                </button>
              </li>
            {/each}
          </ul>
        {:else if listTab === "edges"}
          <ul aria-label="Связи графа">
            {#each edgeRows.slice(0, 400) as e (e.id)}
              <li><button class:on={selected?.type === "edge" && selected.id === e.id} onclick={() => select({ type: "edge", id: e.id })}><span class="name" title={edgeTitle(e)}>{edgeTitle(e)}</span><small>{edgeKindTitle(e)}{e.assertion !== undefined && hasCondition(e.assertion) ? " · условие" : ""}{e.kind === "markdown_link" ? ` · ${linesText(e)}` : e.raw !== null ? ` · cos ${e.raw.toFixed(2)}` : ""}</small></button></li>
            {/each}
          </ul>
        {:else}
          <div class="viewlist">{@render viewDetails()}</div>
        {/if}
      </div>
    {/if}
  </div>

  {#if !mini}
    <aside class="inspector" aria-label="Инспектор графа">
      {#if ctxForm}
        <header><span class="kind">Анализ</span><h3>Контекст задачи</h3></header>
        <label class="field">Задача<textarea rows="3" bind:value={ctxForm.task} placeholder="Что нужно сделать или решить?"></textarea></label>
        <label class="field">Профиль<select bind:value={ctxForm.profile}><option value="implementation">Реализация</option><option value="investigation">Исследование</option><option value="decision">Решение</option></select></label>
        <label class="field">Бюджет, байт<input type="number" min="1024" step="1024" bind:value={ctxForm.budget} /></label>
        <label class="chk"><input type="checkbox" bind:checked={ctxForm.useSubgraph} disabled={!subgraph.size} /> Область — подграф ({subgraphDocCount} документов)</label>
        {#if ctxForm.useSubgraph}<p class="hint">`context.roots` в контракте v1 не реализован: подграф передаётся областью документов.</p>{/if}
        <div class="actions"><button class="act" onclick={runContext}>Собрать контекст</button><button class="act" onclick={() => (ctxForm = null)}>Отмена</button></div>
        <p class="hint">Стенд берёт записанный ответ контекста, ближайший к задаче по словам.</p>
      {:else if selectedNode}
        {@const n = selectedNode}
        {#if tv && tv.kind !== "pick"}<button class="inline back" onclick={() => select(null)}>← к результатам вида «{VIEW_NAME[tv.kind]}»</button>{/if}
        <header>
          <span class="kind">{KIND_HEAD[n.kind]}</span>
          <h3>{n.label}</h3>
          {#if n.kind === "document" || n.kind === "chunk"}<p class="path">{n.path} · модуль {n.module}{#if n.kind === "chunk"} · строки {n.firstLine}–{n.lastLine}{/if}</p>{/if}
          {#if n.kind === "chunk" && n.heading}<p class="path">Раздел: {n.heading.join(" / ")}</p>{/if}
          {#if n.kind === "chunk" && n.roles?.length}<p class="path">Роли: {n.roles.map((r) => ROLE_NAME[r] ?? r).join(", ")}</p>{/if}
          {#if n.kind === "entity"}<p class="path">Определена: {F.entities[n.ref].doc >= 0 ? F.docs[F.entities[n.ref].doc][2] : "—"} · namespace — файл определения{#if F.entities[n.ref].aliases.length} · aliases: {F.entities[n.ref].aliases.join(", ")}{/if}</p>{/if}
          {#if n.kind === "ghost"}<p class="path">Цель ссылки: {n.label} · строка {n.firstLine} · {AUDIT_NAME[F.broken[n.ref][3]] ?? F.broken[n.ref][3]}</p>{/if}
          <p class="state-line" class:warnt={n.stale || n.deleted}>{n.deleted ? "Источник удалён после снимка" : n.stale ? "Изменён после снимка: строки могли сдвинуться" : n.kind === "ghost" ? "Неразрешённая ссылка: цели нет в индексе" : n.ambiguous ? "Неоднозначное имя: два объекта с этим именем" : "Актуален"} <small title="Ревизия снимка">· снимок {F.snapshot.slice(0, 8)}</small></p>
        </header>
        {#if n.kind === "topic"}
          {@const t = F.topics[n.ref]}
          <p class="origin">Название — заголовки представителей темы (сообщество `bars_explore`), не сгенерированный текст. {t.members.length} документов.</p>
          <div class="actions"><button class="act" onclick={() => toggleTopic(n.ref)}>Раскрыть тему</button><button class="act" onclick={() => toggleSubgraph(n.index)}>{subgraph.has(n.index) ? "Из подграфа" : "В подграф"}</button></div>
          <h4>Представители</h4>
          <ul class="conns">{#each t.representatives as d (d)}<li><span class="name">{F.docs[d][0]}</span></li>{/each}</ul>
        {:else}
          <div class="actions">
            {#if n.kind !== "ghost" && n.kind !== "query"}<button class="act" onclick={() => openSource(n.index)} disabled={n.deleted} title={n.deleted ? "Источник удалён после снимка" : "Клавиша O"}><Icon name="doc" size={13} width={1.8} /> {n.kind === "chunk" ? "Открыть диапазон" : n.kind === "entity" ? "Открыть основание" : "Открыть"}</button>{/if}
            {#if n.kind === "document" && n.chunks && lvlOf() === "documents" && !tv && !query}<button class="act" onclick={() => toggleExpand(n.index)}>{expanded[n.index] ? "Свернуть чанки" : `Раскрыть чанки (${n.chunks})`}</button>{/if}
            <button class="act" onclick={() => pin(n.index)} aria-pressed={pinnedSel}>{pinnedSel ? "Открепить" : "Закрепить"}</button>
          </div>
          {#if n.kind !== "query"}
            <h4>Анализ</h4>
            <div class="actions">
              <button class="act" onclick={() => openLocal(n.index)} title="Клавиша L">Локальный граф</button>
              <button class="act" onclick={() => startPath(n.index)} title="Клавиша P">Путь до…</button>
              <button class="act" onclick={() => openImpact(n.index)} disabled={n.kind === "ghost" || scenario === "notbuilt"} title={n.kind === "ghost" ? "У неразрешённой ссылки нет зависимостей" : "Клавиша I"}>Влияние изменения</button>
              <button class="act" onclick={() => toggleSubgraph(n.index)} aria-pressed={subgraph.has(n.index)} title="Shift+пробел или Shift+щелчок">{subgraph.has(n.index) ? "Из подграфа" : "В подграф"}</button>
            </div>
          {/if}
          {#if n.kind === "document" && expanded[n.index] && expanded[n.index] < n.chunks}<p class="trunc">Показано {expanded[n.index]} из {n.chunks} чанков.</p>{/if}
          {#if n.kind === "entity"}
            {@const own = entityAssertions(n.ref)}
            <h4>Утверждения · {own.length}</h4>
            {#each assertionGroups(n.ref) as group (group.title)}
              {#if group.list.length}
                <h5>{group.title}</h5>
                <ul class="conns">
                  {#each group.list as x (x.k)}
                    <li><button onclick={() => selectAssertion(x.k)}><span class="name">{x.a.polarity === "negative" ? "не " : ""}{PREDICATE_NAME[x.a.predicate] ?? x.a.predicate} {x.a.from === n.ref ? "→" : "←"} {F.entities[x.a.from === n.ref ? x.a.to : x.a.from].label}</span><small>{hasCondition(x.k) ? "условие · " : ""}{x.a.evidence.length} осн.</small></button></li>
                  {/each}
                </ul>
              {/if}
            {/each}
            <h4>Упоминания · {entityMentions(n.ref).length}</h4>
            <ul class="conns">{#each entityMentions(n.ref) as [doc, , ev], i (i)}<li><span class="name">{F.docs[doc][0]}</span><small>{ev.length} осн.</small></li>{/each}</ul>
            <p class="hint">Связей {connections.length} на карте; оснований {F.entities[n.ref].evidence.length + own.reduce((s, x) => s + x.a.evidence.length, 0)} — числа не смешиваются.</p>
          {/if}
          {#if n.qrole === "result" && query}
            {@const found = query.results.find((r) => r.node === n.index)}
            {#if found}<p class="origin">Результат поиска №{found.rank} · {SCORE_TEXT[found.kind]} {found.score?.toFixed(2)}.</p>{/if}
          {/if}
          {#if n.kind === "query" && query}
            <h4>Результаты · {query.results.length}</h4>
            <ul class="conns">{#each query.results as r (r.rank)}<li><button disabled={r.node === null} onclick={() => r.node !== null && showResult(r.node)}><span class="name">{r.rank}. {r.title}</span><small>{r.score?.toFixed(2)} · показать в графе</small></button></li>{/each}</ul>
          {/if}
          <h4>Связи на карте · {connections.length}</h4>
          {#each CONN_KINDS as kind (kind)}
            {@const items = connections.filter((c) => c.edge.kind === kind)}
            {#if items.length}
              <h5>{XKIND_TITLE[kind]} · {items.length}</h5>
              <ul class="conns">
                {#each items.slice(0, 30) as c (c.edge.id)}
                  <li><button onclick={() => select({ type: "edge", id: c.edge.id })}><svg width="26" height="10" aria-hidden="true"><line x1="1" y1="5" x2="23" y2="5" stroke={palette?.kindCss[kind]} stroke-width="1.6" stroke-dasharray={dash(kind)} /></svg><span class="name">{kind === "markdown_link" || kind === "assertion" ? (c.edge.a === n.index ? "→ " : "← ") : ""}{label(c.other)}</span><small>{c.edge.raw === null ? (kind === "assertion" && c.edge.assertion !== undefined ? F.assertions[c.edge.assertion].predicate : kind === "aggregate" ? `${Object.values(c.edge.count ?? {}).reduce((a, b) => a + b, 0)} связей` : "ссылка") : c.edge.raw.toFixed(2)}</small></button></li>
                {/each}
              </ul>
            {/if}
          {/each}
          {#if !connections.length}<p class="hint">У узла нет связей в выбранных видах, фильтрах и области. Он остаётся в списке.</p>{/if}
        {/if}
      {:else if selectedEdge}
        {@const e = selectedEdge}
        {#if tv && tv.kind !== "pick"}<button class="inline back" onclick={() => select(null)}>← к результатам вида «{VIEW_NAME[tv.kind]}»</button>{/if}
        <header><span class="kind">{edgeKindTitle(e)}</span><h3>{edgeTitle(e)}</h3></header>
        {#if e.kind === "assertion" && e.assertion !== undefined}
          {@const a = F.assertions[e.assertion]}
          <table aria-label="Утверждение">
            <tbody>
              <tr><th>Предикат</th><td>{a.predicate} · {PREDICATE_NAME[a.predicate]}</td></tr>
              <tr><th>Направление</th><td>{label(e.a)} → {label(e.b)}</td></tr>
              <tr><th>Полярность</th><td>{a.polarity === "negative" ? "отрицание («не»)" : "утверждение"}</td></tr>
              <tr><th>Условия</th><td>{hasCondition(e.assertion) ? (Object.entries(a.qualifiers).map(([k, v]) => `${k}: ${v}`).join("; ") || "условие: без драйвера Vulkan (сценарий стенда)") : "нет"}</td></tr>
              <tr><th>Происхождение</th><td>{ORIGIN[a.origin] ?? a.origin}</td></tr>
              <tr><th>Метод</th><td>{F.method}</td></tr>
            </tbody>
          </table>
          {#if e.conflict}<p class="warnbox"><b>Кандидат конфликта</b> — не установленный дефект: сравните источники и условия обоих утверждений. (Сценарий стенда: пара помечена для показа вида.)</p>{/if}
          <h4>Основания · {a.evidence.length}</h4>
          {#each a.evidence as id (id)}{@const ev = evidenceOf(id)}{#if ev}
            <article class="ev"><p class="path">{ev.docsPath} · строки {ev.first}–{ev.last}</p><blockquote>{ev.quote}</blockquote><button class="inline" onclick={() => openEvidence(id)}>Открыть основание</button></article>
          {/if}{/each}
          {#if e.conflict}
            {@const other = SCENARIO_TARGETS.conflict.find((k) => k !== e.assertion)}
            {#if other !== undefined}<h4>Второе утверждение пары</h4><p class="origin">{F.entities[F.assertions[other].from].label} {PREDICATE_NAME[F.assertions[other].predicate]} {F.entities[F.assertions[other].to].label}</p>{#each F.assertions[other].evidence as id (id)}{@const ev = evidenceOf(id)}{#if ev}<article class="ev"><p class="path">{ev.docsPath} · строки {ev.first}–{ev.last}</p><blockquote>{ev.quote}</blockquote></article>{/if}{/each}{/if}
          {/if}
          <p class="hint">Оценка извлечения не вероятность истины. Отсутствие других утверждений не доказывает отсутствие связи.</p>
        {:else if e.kind === "mention"}
          <p class="origin">Документ упоминает сущность. Упоминание — не утверждение о зависимости.</p>
          {#each e.evidence ?? [] as id (id)}{@const ev = evidenceOf(id)}{#if ev}<article class="ev"><p class="path">{ev.docsPath} · строки {ev.first}–{ev.last}</p><blockquote>{ev.quote}</blockquote><button class="inline" onclick={() => openEvidence(id)}>Открыть основание</button></article>{/if}{/each}
        {:else if e.kind === "aggregate"}
          <p class="origin">Связи между темами, свёрнутые в одну линию: {Object.values(e.count ?? {}).reduce((a, b) => a + b, 0)} связей — {Object.entries(e.count ?? {}).map(([k, v]) => `${XKIND_TITLE[k as XKind].toLowerCase()} ${v}`).join(", ")}. Раскройте тему, чтобы увидеть исходные связи и их основания.</p>
          <div class="actions">{#each [e.a, e.b] as end (end)}{#if nodeOf(end).kind === "topic"}<button class="act" onclick={() => toggleTopic(nodeOf(end).ref)}>Раскрыть «{label(end).slice(0, 18)}»</button>{/if}{/each}</div>
        {:else if e.kind === "unresolved"}
          <p class="origin">Ссылка из «{label(e.a)}» (строка {e.lines[0]}) ведёт на «{label(e.b)}», которого нет в индексе. Узел-призрак не выдумывает документ: это статус ссылки.</p>
          <div class="actions"><button class="act" onclick={() => openSource(e.a, e.lines[0])}>Строка ссылки</button></div>
        {:else}
          <p class="origin">{e.kind === "markdown_link" ? `Явная Markdown-ссылка (${linesText(e)}).` : e.kind === "document_similarity" ? "Близость документов: максимум cosine по парам их чанков (снимок стенда), k=5." : e.kind === "chunk_similarity" ? "Близость двух чанков по готовым векторам." : e.kind === "sequence" ? "Порядок чанков внутри документа: служебная линия группы." : ""} Близость не доказывает зависимость.</p>
          <table aria-label="Числа связи"><tbody>
            <tr><th>Исходный cosine</th><td>{e.raw === null ? "нет" : e.raw.toFixed(3)}</td></tr>
            <tr><th>Сила s</th><td>{e.s.toFixed(3)}</td></tr>
            <tr><th>Толщина</th><td>{widthOf(drawStrength(e as never)).toFixed(2)} px</td></tr>
            <tr><th>Яркость</th><td>{Math.round(brightnessOf(drawStrength(e as never)) * 100)}%</td></tr>
            <tr><th>Притяжение q(s)</th><td>{springOf(e.s).toFixed(3)}</td></tr>
          </tbody></table>
          {#if e.kind === "markdown_link" && e.lines.length}<div class="actions"><button class="act" onclick={() => openSource(e.a, e.lines[0])}><Icon name="doc" size={13} width={1.8} /> Источник ссылки</button></div>{/if}
        {/if}
      {:else if tv && tv.kind !== "pick"}
        {@render viewDetails()}
      {:else}
        <header><span class="kind">Граф проекта</span><h3>Выберите узел или связь</h3></header>
        <p class="hint">Инспектор показывает тип, состояние, основания и действия анализа. Движение камеры область поиска не меняет.</p>
        <table aria-label="Состав"><tbody>
          <tr><th>Уровень</th><td>{LEVEL_NAME[lvlOf()]}</td></tr>
          <tr><th>Документов</th><td>{slice.docCount} из {F.docs.length}</td></tr>
          <tr><th>Чанков</th><td>{F.chunks.length}</td></tr>
          <tr><th>Сущностей / утверждений</th><td>{F.entities.length} / {F.assertions.length}</td></tr>
          <tr><th>Смысловой слой</th><td>{scenario === "lagging" ? "отстаёт · частично" : "частично"}</td></tr>
          <tr><th>Снимок</th><td>{F.snapshot.slice(0, 8)} · {F.captured}</td></tr>
        </tbody></table>
        {#if fileErrors.length}
          <h4>Не обработаны файлы · {fileErrors.length}</h4>
          <ul class="conns">{#each fileErrors as [path, why] (path)}<li><span class="name" title={path}>{path}</span><small>{why}</small></li>{/each}</ul>
          <p class="hint">Готовые слои остальных файлов работают; ошибка одного файла не делает граф пустым.</p>
        {/if}
        <p class="keys"><kbd>V</kbd> уровень <kbd>M</kbd> режим <kbd>L</kbd> локальный <kbd>P</kbd> путь <kbd>[ ]</kbd> варианты <kbd>I</kbd> влияние <kbd>O</kbd> открыть <kbd>⇧ Пробел</kbd> подграф <kbd>⌫</kbd> общий вид <kbd>Esc</kbd> отмена</p>
        <p class="hint">Стенд: данные — замороженный корпус проверки качества {F.captured} ({F.docs.length} документов); операции воркера повторены по снимку.</p>
      {/if}
      {#if notice}<p class="notice" role="status">{notice}</p>{/if}
    </aside>
  {/if}
</div>
</div>

{#snippet viewDetails()}
  {#if tv?.kind === "path"}
    {@const p = tv.alts[tv.at]}
    <header><span class="kind">Путь по карте</span><h3>«{label(tv.from)}» → «{label(tv.to)}»</h3></header>
    {#if p}
      <ol class="steps">
        {#each p.steps as s, i (i)}
          <li><button onclick={() => select({ type: "edge", id: s.edge.id })}><b>{i + 1}.</b> {label(p.nodes[i])} {s.forward ? "→" : "←"} {label(p.nodes[i + 1])}</button><small>{edgeKindTitle(s.edge)}{s.edge.kind === "markdown_link" ? ` · ${linesText(s.edge)}` : s.edge.raw !== null ? ` · cos ${s.edge.raw.toFixed(2)}` : ""}</small></li>
        {/each}
      </ol>
      <p class="hint">Варианты: {tv.alts.map((a, i) => `${i + 1} — ${a.steps.length} шаг.`).join(", ")}. Клавиши [ и ].</p>
    {:else}
      <p class="warnbox">В режиме «{MODE_NAME[mode]}» пути нет (до 6 шагов).</p>
    {/if}
    <h4>Смысловой путь (bars_paths)</h4>
    <div class="actions"><button class="act" aria-pressed={tv.sem !== null && tv.semKind === "semantic"} onclick={() => semanticPath("semantic")}>Смысловой</button><button class="act" aria-pressed={tv.sem !== null && tv.semKind === "mixed"} onclick={() => semanticPath("mixed")}>Со структурными шагами</button></div>
    {#if tv.sem}
      {#if tv.sem.paths.length}
        {#each tv.sem.paths as sp, i (i)}
          <h5>Путь {i + 1} · {sp.steps.length} шагов</h5>
          <ol class="steps">{#each sp.steps as s, j (j)}<li><span><b>{j + 1}.</b> {s.label}</span><small>{s.kind === "assertion" ? `предикат ${s.predicate}${s.reversed ? " · обратный обход" : ""}` : s.kind === "mention" ? "упоминание" : `ссылка, строки ${s.lines.join(", ")}`}</small>{#each s.evidence.slice(0, 1) as id (id)}{@const ev = evidenceOf(id)}{#if ev}<blockquote>{ev.quote}</blockquote><button class="inline" onclick={() => openEvidence(id)}>Основание шага</button>{/if}{/each}</li>{/each}</ol>
        {/each}
      {:else}
        <p class="warnbox">{tv.sem.reason}</p>
      {/if}
      <p class="hint">Область поиска: {tv.sem.searched}.</p>
    {/if}
  {:else if tv?.kind === "impact"}
    <header><span class="kind">Влияние изменения</span><h3>«{label(tv.root)}»</h3></header>
    {#if tv.res.note}<p class="warnbox">{tv.res.note}</p>{/if}
    {#each [{ title: "Прямое", items: tv.res.direct }, { title: "Косвенное", items: tv.res.indirect }] as group (group.title)}
      {#if group.items.length}
        <h4>{group.title} · {group.items.length}</h4>
        <ul class="conns">{#each group.items as it (it.id)}<li><button onclick={() => { const i = nodeFor(it.id); if (i !== undefined) select({ type: "node", index: i }); }}><span class="name">Проверить описание: {it.label}</span><small>шаг {it.depth}</small></button><small class="sub">{it.path.map((s) => s.label).join(" ← ")}</small></li>{/each}</ul>
      {/if}
    {/each}
    <h4>Ссылки-соседи · {tv.res.neighbours.length}</h4>
    <p class="hint">Ссылаются на объект или он на них — это не доказанная зависимость.</p>
    <ul class="conns">{#each tv.res.neighbours as nb, i (i)}<li><button onclick={() => { const j = xs.indexOf(`doc:${nb.doc}`); if (j !== undefined) select({ type: "node", index: j }); }}><span class="name">{nb.incoming ? "← " : "→ "}{F.docs[nb.doc][0]}</span><small>строки {nb.lines.slice(0, 4).join(", ")}</small></button></li>{/each}</ul>
  {:else if tv?.kind === "context"}
    <header><span class="kind">Контекст задачи · {tv.scope}</span><h3>{tv.task || tv.reply.query}</h3></header>
    {#if tv.task && tv.task !== tv.reply.query}<p class="hint">Стенд показал записанный ответ на задачу «{tv.reply.query}».</p>{/if}
    {#each tv.reply.groups as g (g.role)}
      <h4><span class="rl">{ROLE_LETTER[g.role]}</span> {ROLE_NAME[g.role] ?? g.role} · {g.evidence.length}</h4>
      {#each g.evidence.slice(0, 3) as id (id)}{@const ev = evidenceOf(id)}{#if ev}<article class="ev"><p class="path">{ev.docsPath} · строки {ev.first}–{ev.last}</p><blockquote>{ev.quote}</blockquote><button class="inline" onclick={() => openEvidence(id)}>Источник роли</button></article>{/if}{/each}
    {/each}
    {#if tv.reply.missing.length}<p class="warnbox">Не найдены роли: {tv.reply.missing.map((r) => ROLE_NAME[r] ?? r).join(", ")}. Неполная обработка не доказывает отсутствие источников.</p>{/if}
    <p class="hint">Использовано {(tv.reply.used / 1024).toFixed(1)} КБ; режим бюджета {tv.reply.mode === "estimated" ? "оценочный (tokenizer не выбран)" : "точный"}.</p>
  {:else if tv?.kind === "topics"}
    <header><span class="kind">Обзор тем</span><h3>{F.topics.length} тем · сообщества</h3></header>
    <ul class="conns">{#each F.topics as t, i (i)}<li><button onclick={() => toggleTopic(i)}><span class="name" title={t.label}>{t.label.split(" / ")[0]}</span><small>{t.members.length} док. · {tv.open.has(i) ? "свернуть" : "раскрыть"}</small></button></li>{/each}</ul>
    <p class="hint">Связи между темами агрегированы по виду с числом; число связей и число оснований не смешиваются.</p>
  {:else if tv?.kind === "audit"}
    <header><span class="kind">Аудит связей</span><h3>Структурный слой · S8</h3></header>
    {#each Object.keys(AUDIT_NAME) as kind (kind)}
      {@const items = F.audit.filter((a) => a.kind === kind)}
      {#if items.length}
        <h4>{AUDIT_NAME[kind]} · {items.length}</h4>
        <ul class="conns">{#each items.slice(0, 12) as a, i (i)}<li><button onclick={() => { if (a.doc !== null) { const j = xs.indexOf(`doc:${a.doc}`); if (j !== undefined) select({ type: "node", index: j }); } }}><span class="name">{a.doc !== null ? F.docs[a.doc][0] : "—"}</span><small title={a.detail}>{a.detail.slice(0, 40)}</small></button></li>{/each}</ul>
      {/if}
    {/each}
    <h4>Хабы</h4>
    <ul class="conns">{#each hubs() as h (h.doc)}<li><span class="name">{F.docs[h.doc][0]}</span><small>{h.degree} ссылок</small></li>{/each}</ul>
    <p class="hint">Кандидаты пробелов и конфликтов — кандидаты, не вердикты. Расписание, игнорирование и новые темы — экран аудитов (этап 3.8).</p>
  {:else if tv?.kind === "local"}
    <header><span class="kind">Локальный граф</span><h3>«{label(tv.center)}»</h3></header>
    {@const maxDepth = tv.depth}
    {#each [1, 2, 3].filter((d) => d <= maxDepth) as d (d)}
      {@const ring = [...tv.res.depthOf].filter(([, x]) => x === d)}
      <h4>Шаг {d} · {ring.length}</h4>
      <ul class="conns">{#each ring.slice(0, 20) as [i] (i)}<li><button onclick={() => select({ type: "node", index: i })}><span class="name">{label(i)}</span></button></li>{/each}</ul>
    {/each}
  {/if}
{/snippet}

<style>
  .pgw { display: flex; flex: 1; min-width: 0; min-height: 0; height: 100%; container-type: inline-size; }
  .pg { position: relative; flex: 1; display: grid; grid-template-columns: minmax(0, 1fr) 310px; grid-template-rows: auto minmax(0, 1fr); min-height: 0; min-width: 0; height: 100%; background: var(--panel); font-size: 12px; color: var(--text); overflow: hidden; }
  .pg.mini { display: flex; flex-direction: column; }
  .tops { grid-column: 1 / -1; display: flex; flex-direction: column; }
  .bar { display: flex; align-items: center; gap: 6px 8px; padding: 4px 8px; border-bottom: 1px solid var(--line); flex-wrap: wrap; }
  .banner, .vbar, .qbar, .sbar { display: flex; gap: 6px 12px; align-items: center; flex-wrap: wrap; padding: 4px 10px; border-bottom: 1px solid var(--line); font-size: 11.5px; }
  .banner { background: var(--mid-soft); }
  .banner.warn { background: color-mix(in srgb, #d9730d 14%, var(--panel)); }
  .banner.subtle { background: var(--panel-2); color: var(--muted); }
  .vbar { background: color-mix(in srgb, var(--accent) 12%, var(--panel)); }
  .qbar { background: var(--accent-soft); }
  .sbar { background: var(--panel-2); }
  .vic { width: 10px; height: 10px; border-radius: 50%; border: 2px solid var(--accent); flex: none; }
  .sq { width: 9px; height: 9px; background: var(--accent); border-radius: 2px; flex: none; }
  .vt, .qtext { max-width: 46ch; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .more { border: 1px solid var(--line-2); background: var(--panel); border-radius: 5px; padding: 1px 8px; font-size: 11.5px; color: var(--text); }
  .vbar > .more:last-child, .qbar > .more:last-child { margin-left: auto; }
  .more.ghost { background: transparent; }
  .more:disabled { opacity: 0.5; }
  .depth, .alt { display: inline-flex; align-items: center; gap: 4px; }
  .depth button, .alt button { min-width: 20px; height: 20px; border: 1px solid var(--line-2); background: var(--panel); border-radius: 4px; font-size: 11px; color: var(--text); }
  .depth button.on { background: var(--accent); border-color: var(--accent); color: #fff; }
  .depth button:disabled, .alt button:disabled { opacity: 0.4; }
  .qsq { display: inline-block; width: 9px; height: 9px; background: #d9730d; flex: none; }
  .warnt { color: #c2410c; }
  .ring { display: inline-block; width: 9px; height: 9px; border-radius: 50%; border: 2px solid #d9730d; margin-right: 3px; vertical-align: -1px; }
  .ring.indirect { border-style: dashed; }
  .ring.neighbour, .ring.isolate { border-color: var(--muted); }
  .ring.isolate { border-style: dashed; }
  .ring.hub { border-color: #5b6cd6; }
  .stage { grid-column: 1; grid-row: 2; position: relative; min-width: 0; min-height: 0; overflow: hidden; background: var(--panel); }
  .pg.mini .stage { flex: 1; }
  canvas { position: absolute; inset: 0; width: 100%; height: 100%; display: block; cursor: grab; touch-action: none; outline: none; }
  canvas.hide { visibility: hidden; }
  canvas:focus-visible { box-shadow: inset 0 0 0 2px var(--accent); }
  .find { display: flex; align-items: center; gap: 4px; height: 24px; padding: 0 6px; border: 1px solid var(--line-2); border-radius: 6px; color: var(--muted); }
  .find input { border: 0; background: transparent; outline: 0; width: 130px; font-size: 11px; color: var(--text); }
  .found { font-size: 10.5px; color: var(--accent-text); }
  .seg { display: flex; flex-wrap: wrap; }
  .mode { display: inline-flex; align-items: center; gap: 5px; height: 24px; padding: 0 8px; border: 1px solid var(--line-2); background: var(--panel); color: var(--muted); font-size: 11px; margin-left: -1px; }
  .seg .mode:first-child { border-radius: 6px 0 0 6px; margin-left: 0; }
  .seg .mode:last-child { border-radius: 0 6px 6px 0; }
  .mode.solo { border-radius: 6px; margin: 0; }
  .mode[aria-checked="true"], .mode[aria-expanded="true"] { background: var(--accent-soft); color: var(--accent-text); border-color: var(--accent); position: relative; z-index: 1; }
  .mode b { font-weight: 600; }
  .mode .state { font-size: 9.5px; padding: 0 4px; border-radius: 8px; background: var(--mid-soft); color: var(--mid); }
  .shape { display: inline-block; width: 8px; height: 8px; flex: none; background: var(--muted); border-radius: 50%; }
  .shape.chunks { border-radius: 1px; transform: rotate(45deg) scale(0.85); }
  .shape.entities { border-radius: 0; clip-path: polygon(25% 5%, 75% 5%, 100% 50%, 75% 95%, 25% 95%, 0 50%); background: #0f8a83; }
  .shape.mixed { background: linear-gradient(90deg, var(--muted) 50%, #0f8a83 50%); }
  .shape.ghost { background: transparent; border: 1px dashed #c23a3a; }
  .shape.topic { width: 11px; height: 11px; background: #5b6cd6; }
  .shape.query { border-radius: 0; background: #d9730d; }
  .sep { flex: 1; }
  .tools { display: flex; gap: 1px; padding: 2px; border: 1px solid var(--line); border-radius: 5px; background: var(--panel); }
  .tools button { position: relative; display: grid; place-items: center; width: 20px; height: 20px; padding: 0; border: 0; border-radius: 4px; background: transparent; color: var(--muted); }
  .tools button:hover, .tools button[aria-pressed="true"], .tools button[aria-expanded="true"] { background: var(--accent-soft); color: var(--accent-text); }
  .tools button:disabled { opacity: 0.4; }
  .badged i { position: absolute; right: -3px; top: -3px; min-width: 11px; height: 11px; border-radius: 6px; background: var(--accent); color: #fff; font: 600 8px/11px system-ui; font-style: normal; }
  .menu-btn { height: 24px; border: 1px solid var(--line-2); border-radius: 6px; background: var(--panel); color: var(--text); font-size: 11px; padding: 0 8px; }
  .menu-btn[aria-expanded="true"] { background: var(--accent-soft); border-color: var(--accent); }
  button:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .toast { position: absolute; left: 6px; top: 6px; max-width: calc(100% - 150px); padding: 4px 10px; border-radius: 8px; background: var(--text); color: var(--panel); font-size: 11.5px; box-shadow: var(--shadow); pointer-events: none; z-index: 6; }
  .status { position: absolute; left: 8px; bottom: 6px; display: flex; align-items: center; gap: 6px; font-size: 10.5px; color: var(--muted); background: color-mix(in srgb, var(--panel) 85%, transparent); padding: 1px 6px; border-radius: 6px; max-width: calc(100% - 16px); }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--faint); flex: none; }
  .dot.running { background: var(--accent); }
  .dot.stopped { background: var(--mid); }
  .inline { border: 0; background: none; padding: 0; color: var(--accent-text); font-size: inherit; text-decoration: underline; text-underline-offset: 2px; }
  .inline.back { display: block; margin-bottom: 6px; font-size: 11px; }
  .tip { position: absolute; max-width: 260px; padding: 3px 7px; border-radius: 6px; background: var(--text); color: var(--panel); font-size: 11px; pointer-events: none; z-index: 3; }
  .caption { position: absolute; left: 6px; right: 6px; bottom: 24px; display: flex; align-items: center; gap: 8px; padding: 3px 8px; border: 1px solid var(--line); border-radius: 6px; background: var(--panel); font-size: 11px; }
  .caption b { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .empty { position: absolute; inset: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 6px; padding: 20px; text-align: center; color: var(--muted); margin: 0; z-index: 2; }
  .note { position: absolute; top: 8px; left: 8px; right: 8px; margin: 0; padding: 4px 8px; border-radius: 6px; background: var(--mid-soft); color: var(--text); font-size: 11.5px; }
  .qplate { position: absolute; left: 6px; top: 4px; display: flex; align-items: center; gap: 6px; padding: 2px 8px; border: 1px dashed var(--line-2); border-radius: 6px; background: var(--panel); font-size: 11px; }
  .legend, .pop { position: absolute; top: 8px; right: 8px; width: 270px; max-height: calc(100% - 16px); overflow: auto; padding: 8px 10px; border: 1px solid var(--line-2); border-radius: 8px; background: var(--panel); box-shadow: var(--shadow); z-index: 5; }
  .pop.menu { display: flex; flex-direction: column; gap: 2px; }
  .pop.menu button { display: grid; text-align: left; gap: 1px; border: 0; background: transparent; border-radius: 6px; padding: 5px 6px; color: var(--text); font-size: 11.5px; }
  .pop.menu button:hover { background: var(--accent-soft); }
  .pop.menu small { color: var(--muted); font-size: 10.5px; }
  .legend h4, .inspector h4, .pop h4, .viewlist h4 { margin: 8px 0 3px; font-size: 10.5px; color: var(--muted); font-weight: 600; }
  .legend h4:first-child, .pop h4:first-child { margin-top: 0; }
  .legend .row { display: flex; align-items: center; gap: 6px; margin: 3px 0; }
  .legend svg { color: var(--muted); fill: var(--muted); flex: none; }
  .opt { display: flex; align-items: center; gap: 6px; margin: 3px 0; font-size: 11px; }
  .opt span { flex: 1; }
  .opt input[type="range"] { width: 70px; accent-color: var(--accent); }
  .chk { display: flex; align-items: center; gap: 5px; font-size: 11px; color: var(--text); margin: 2px 0; }
  .chk small { color: var(--faint); margin-left: auto; }
  .radios { display: flex; gap: 10px; flex-wrap: wrap; }
  .reset { margin-top: 6px; border: 1px solid var(--line-2); background: transparent; color: var(--muted); border-radius: 5px; padding: 1px 8px; font-size: 11px; }
  .hint { margin: 4px 0; color: var(--muted); font-size: 11px; line-height: 1.4; }
  .inspector { grid-column: 2; grid-row: 2; min-height: 0; overflow: auto; padding: 8px 10px; border-left: 1px solid var(--line); background: var(--panel); }
  .inspector header { margin-bottom: 6px; }
  .kind { font-size: 10.5px; text-transform: uppercase; letter-spacing: 0.04em; color: var(--accent-text); font-weight: 600; }
  .inspector h3, .viewlist h3 { margin: 1px 0 2px; font-size: 13px; line-height: 1.3; overflow-wrap: anywhere; }
  .inspector h5, .viewlist h5 { margin: 6px 0 2px; font-size: 11px; color: var(--muted); font-weight: 600; }
  .path { margin: 0; color: var(--muted); font-size: 11px; overflow-wrap: anywhere; }
  .state-line { margin: 3px 0 0; font-size: 11px; }
  .state-line small { color: var(--faint); }
  .actions { display: flex; gap: 4px; flex-wrap: wrap; margin: 6px 0; }
  .act { display: inline-flex; align-items: center; gap: 5px; padding: 2px 8px; height: 24px; border: 1px solid var(--line-2); border-radius: 6px; background: transparent; color: var(--text); font-size: 11px; }
  .act:hover, .act[aria-pressed="true"] { background: var(--accent-soft); color: var(--accent-text); border-color: transparent; }
  .act:disabled { opacity: 0.45; }
  .conns, .steps { list-style: none; margin: 0; padding: 0; }
  .steps li { margin: 3px 0; }
  .conns button, .list li button, .steps button { display: flex; align-items: center; gap: 6px; width: 100%; padding: 2px 4px; border: 0; border-radius: 5px; background: transparent; text-align: left; font-size: 11.5px; color: var(--text); }
  .conns button:hover, .list li button:hover, .steps button:hover { background: var(--panel-2); }
  .conns li { display: flex; flex-direction: column; }
  .conns li > span.name { padding: 2px 4px; }
  .conns .name, .list .name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .conns small, .list small, .steps small { color: var(--muted); font-size: 10.5px; }
  .conns small.sub { padding: 0 4px 3px; }
  .origin { margin: 4px 0; line-height: 1.45; }
  .ev { margin: 6px 0; padding: 4px 6px; border-left: 2px solid var(--line-2); }
  blockquote { margin: 3px 0; font-size: 11px; color: var(--text); white-space: pre-wrap; overflow-wrap: anywhere; max-height: 7.5em; overflow: auto; }
  .warnbox { margin: 6px 0; padding: 4px 6px; border-radius: 5px; background: color-mix(in srgb, #d9730d 14%, var(--panel)); font-size: 11px; line-height: 1.4; }
  .rl { display: inline-grid; place-items: center; min-width: 16px; height: 13px; border-radius: 6px; background: var(--text); color: var(--panel); font-size: 9px; }
  table { width: 100%; border-collapse: collapse; margin: 6px 0; font-size: 11.5px; }
  th { text-align: left; font-weight: 500; color: var(--muted); padding: 2px 6px 2px 0; }
  td { text-align: right; overflow-wrap: anywhere; }
  tr + tr { border-top: 1px solid var(--line); }
  .trunc { margin: 4px 0; padding: 3px 6px; border-radius: 5px; background: var(--mid-soft); font-size: 11px; }
  .notice { margin: 8px 0 0; padding: 4px 6px; border-radius: 5px; background: var(--accent-soft); color: var(--accent-text); font-size: 11px; }
  .keys { color: var(--muted); font-size: 10.5px; line-height: 2; }
  .field { display: grid; gap: 2px; margin: 4px 0; font-size: 11px; color: var(--muted); }
  .field textarea, .field select, .field input { font: inherit; font-size: 11.5px; color: var(--text); background: var(--panel); border: 1px solid var(--line-2); border-radius: 5px; padding: 3px 5px; }
  .list { position: absolute; inset: 0; display: flex; flex-direction: column; min-height: 0; }
  .list-tools { display: flex; gap: 8px; align-items: center; padding: 6px 8px; border-bottom: 1px solid var(--line); flex-wrap: wrap; }
  .list-tools input[type="search"] { flex: 1; min-width: 120px; height: 24px; border: 1px solid var(--line-2); border-radius: 6px; padding: 0 6px; background: transparent; font-size: 11px; color: var(--text); }
  .tabs { display: flex; }
  .tabs button { height: 24px; border: 1px solid var(--line-2); background: var(--panel); color: var(--muted); font-size: 11px; padding: 0 8px; margin-left: -1px; }
  .tabs button:first-child { border-radius: 6px 0 0 6px; margin-left: 0; }
  .tabs button:last-child { border-radius: 0 6px 6px 0; }
  .tabs button[aria-selected="true"] { background: var(--accent-soft); color: var(--accent-text); border-color: var(--accent); position: relative; }
  .tabs button:disabled { opacity: 0.45; }
  .list .note { position: static; margin: 6px 8px; }
  .list ul { list-style: none; margin: 0; padding: 4px 6px; overflow: auto; flex: 1; }
  .list li { display: flex; align-items: center; gap: 4px; }
  .list li button.on { background: var(--accent-soft); color: var(--accent-text); }
  .list em { font-style: normal; font-size: 10px; padding: 0 5px; border-radius: 8px; background: var(--mid-soft); color: var(--mid); }
  .viewlist { overflow: auto; padding: 8px 10px; flex: 1; }
  @container (max-width: 640px) {
    .pg:not(.mini) { grid-template-columns: minmax(0, 1fr); grid-template-rows: auto minmax(0, 1fr) minmax(0, 40%); }
    .inspector { grid-column: 1; grid-row: 3; border-left: 0; border-top: 1px solid var(--line); }
    .find input { width: 90px; }
    .vt, .qtext { max-width: 28ch; }
  }
</style>
