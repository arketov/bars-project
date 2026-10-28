<script lang="ts">
  // Физический граф проекта (пункт плана 2.1.5): одна панель для мини-графа под навигацией и для вкладки «Граф».
  // Данные — срез `graph_physical` (physical-v1), физика — Web Worker, отрисовка — Canvas2D. Дизайн: docs/features/graphrag/physical-graph-design.md.
  import { onDestroy, onMount, untrack } from "svelte";
  import GraphSearchBundle from "../GraphSearchBundle.svelte";
  import Icon from "../Icon.svelte";
  import { app } from "../app.svelte";
  import { appearance } from "../appearance.svelte";
  import { documentColor, keyOf } from "../appearance";
  import type { GraphStatus } from "../graph";
  import { graphDataLabel, completenessLabels, jobLabels } from "../graph";
  import { BarsError } from "../types";
  import { draw, layoutLines, lineDistance, moduleColors, radiusOf, readPalette, toScreen, toWorld, type Camera, type Palette, type Scene, type Target } from "./draw";
  import { DEFAULT_FORCES, Layout, type Forces, type PNode } from "./layout";
  import { EDGE_KINDS, KIND_TITLE, brightnessOf, drawStrength, springOf, widthOf, type EdgeKind, type GEdge, type GNode, type Mode, type SliceKind } from "./model";
  import { graphStatus, loadPhysical, loadQuery, startGraphBuild, stopGraphBuild } from "./physical-api";
  import { GRAPH_ACTIONS, NO_SELECTION_TEXT, actionState, buildControls } from "./entry";
  import { FORCE_RANGE, defaultSettings, loadSettings, saveSettings, type PanelKind, type PanelSettings } from "./settings";
  import { INITIAL_LIMITS, MAX_LIMITS, PORTION, SliceStore, emptySlice, type Limits, type Slice } from "./slice";
  import { DEFAULT_DEPTH, MAX_DEPTH, QUERY_BUDGET, QUERY_MAX, SEARCH_MODE, acceptQuerySlice, queryRequest, fallbackText, shapeQuery, type QueryBuild, type QueryInfo, type QueryInput, type RetrievalKind, type ScoreKind } from "./query";

  const reduced = typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;

  let host: HTMLDivElement;
  let stage: HTMLDivElement;
  let canvas = $state<HTMLCanvasElement | null>(null);
  let mini = $state(false);
  let mounted = $state(false);
  let size = $state({ w: 0, h: 0 });

  /** Режим связей, порог близости и раскладка: отдельно для мини-графа и полного графа, по проекту (решение OQ-118). */
  let settings = $state<PanelSettings>(defaultSettings());
  const mode = $derived(settings.mode);
  const minS = $derived(settings.minS);
  const forces = $derived(settings.forces);
  const modeKinds = (m: Mode): SliceKind[] => (m === "links" ? ["markdown_link"] : m === "meaning" ? ["document_similarity"] : m === "chunks" ? ["chunk_similarity"] : ["markdown_link", "document_similarity", "chunk_similarity"]);
  const kinds = $derived.by(() => {
    const on = modeKinds(mode);
    return { markdown_link: on.includes("markdown_link"), document_similarity: on.includes("document_similarity"), chunk_similarity: on.includes("chunk_similarity"), query_result: true } as Record<EdgeKind, boolean>;
  });

  let store = new SliceStore();
  /** Общий срез проекта; `slice` — то, что рисуется: общий срез или он же с узлом запроса и его связями (вид запроса, Т-47). */
  let base = $state.raw<Slice>(emptySlice());
  let slice = $state.raw<Slice>(emptySlice());
  /** Активный вид запроса; null — общий вид. */
  let query = $state.raw<QueryInfo | null>(null);
  let queryDepth = $state(DEFAULT_DEPTH);
  let queryBudget = $state({ ...QUERY_BUDGET });
  /** Поиск, для которого вид уже включался, и поиск, из которого пользователь вернулся в общий вид. */
  let generalState: { positions: ReturnType<Layout["save"]>; camera: Camera; w: number; h: number; selection: Target; expanded: Record<number, number>; stopped: boolean } | null = null;
  let packageIndex = $state<number | null>(null);
  let queryKey = "";
  let queryOff = $state("");
  let queryEpoch = 0;
  let queryBuilt: QueryBuild | null = null;
  let queryError = $state("");
  let queryLoading = $state(false);
  let queryRequestKey = "";
  let loaded = $state(false);
  /** Вкладка уже открылась на объекте (или решила не открываться): повторно выбор по активному документу не выставляется. */
  let openedOnObject = false;
  let loading = $state(false);
  let loadError = $state("");
  let loadErrorKind = $state("");
  let status = $state<GraphStatus | null>(null);
  let limits: Limits = { ...INITIAL_LIMITS };
  let epoch = 0;
  let statusTimer: ReturnType<typeof setInterval> | undefined;

  let toast = $state("");
  let toastTimer = 0;
  let expanded = $state<Record<number, number>>({});
  const shownChunks = $derived(Object.values(expanded).reduce((a, b) => a + b, 0));
  let selected = $state<Target>(null);
  let tip = $state<{ text: string; x: number; y: number } | null>(null);
  let motion = $state<"running" | "idle" | "stopped" | "reduced">(reduced ? "reduced" : "idle");
  let stopped = $state(false);
  let view = $state<"map" | "list">("map");
  let listFilter = $state("");
  let isolatesOnly = $state(false);
  let findText = $state("");
  let legendOpen = $state(false);
  /** Открытое меню: настройки раскладки. */
  let popover = $state<"settings" | "more" | null>(null);
  let notice = $state("");
  let rendererError = $state("");
  let revision = $state(0);

  let layout = $state.raw<Layout | null>(null);
  let palette = $state.raw<Palette | null>(null);
  let cam: Camera = { x: 0, y: 0, k: 1 };
  let fitted = false;
  /** После первой раскладки камера сама вписывает растущий граф, пока пользователь её не тронул (не дольше AUTO_FIT_MS). */
  let autoFitSince = 0;
  let lastAutoFit = 0;
  const AUTO_FIT_MS = 30000;
  /** Куда плавно едет камера при автовписывании; ручное действие отменяет движение. */
  let camTarget: Camera | null = null;
  let raf = 0;
  let hidden = false;
  let hovered: Target = null;
  let hostObserver: ResizeObserver | null = null;
  let themeObserver: MutationObserver | null = null;
  let io: IntersectionObserver | null = null;

  // Производные сравниваются по значению: перечитывание списка проектов не пересоздаёт модель графа.
  const projectPath = $derived(app.project?.path ?? null);
  const scopeKey = $derived(JSON.stringify(app.scope));
  const panelKind = (): PanelKind => (mini ? "mini" : "full");
  const project = () => app.project?.path ?? null;
  const owner = (index: number) => slice.nodes[index].owner;
  const label = (index: number) => slice.nodes[index].label;
  const chunksOfDoc = (index: number): number[] => slice.chunksOf.get(index) ?? [];

  /** Центр локального графа мини-панели: последний активный документ, если он есть в срезе (Т-43, раунд 27). */
  const focusIndex = $derived.by(() => {
    const path = app.activeDocument;
    if (!mini || !path) return null;
    const n = base.nodes.find((x) => x.kind === "document" && x.path === path && base.visibleDocs.has(x.index));
    return n ? n.index : null;
  });
  /** Мини-панель показывает локальный граф документа; вид запроса после поиска важнее. */
  const isLocal = () => mini && !query && focusIndex !== null;

  /** Документ и его соседи по видимым связям режима на глубину `depth`; ссылки — в обе стороны. */
  function localDocs(): Set<number> {
    const set = new Set<number>([focusIndex!]);
    let frontier = new Set<number>(set);
    for (let step = 0; step < settings.depth && frontier.size; step++) {
      const next = new Set<number>();
      for (const e of slice.edges) {
        if (!kinds[e.kind] || e.kind === "chunk_similarity" || !passes(e)) continue;
        if (frontier.has(e.a) && !set.has(e.b) && slice.visibleDocs.has(e.b)) next.add(e.b);
        if (frontier.has(e.b) && !set.has(e.a) && slice.visibleDocs.has(e.a)) next.add(e.a);
      }
      for (const index of next) set.add(index);
      frontier = next;
    }
    return set;
  }

  function visibleNodes() {
    if (queryShape) return queryShape.nodes.map((index) => slice.nodes[index]);
    if (isLocal()) {
      const docs = localDocs();
      return slice.nodes.filter((n) => n.kind === "document" && docs.has(n.index));
    }
    const nodes = slice.nodes.filter((n) => n.kind === "document" && slice.visibleDocs.has(n.index));
    if (!mini) for (const [o, shown] of Object.entries(expanded)) for (const c of chunksOfDoc(Number(o)).slice(0, shown)) nodes.push(slice.nodes[c]);
    return nodes;
  }

  const passes = (e: GEdge) => e.kind === "markdown_link" || e.kind === "query_result" || e.s >= minS;

  /** Что известно о завершённом поиске окна; null — поиска нет, он идёт, закончился ошибкой или относится к другому проекту. */
  function queryInput(): QueryInput | null {
    const results = app.results;
    const shown = app.displayed;
    if (!results || !shown || shown.project !== project()) return null;
    if (JSON.stringify({ modules: shown.modules, files: shown.files }) !== JSON.stringify(app.scope)) return null;
    const graph = app.graphSearch;
    const requested = shown.mode as RetrievalKind;
    const applied = (graph?.applied_mode ?? requested) as RetrievalKind;
    const scoreKind: ScoreKind = applied === "graph" ? "none" : results.applied.score_kind;
    return { id: shown.requestId, text: shown.query, hits: results.results, bundle: graph?.related ?? graph?.graph_results, scope: { modules: shown.modules, files: shown.files }, scoreKind, requested, applied, fallback: graph?.fallback_reason ?? null, minScore: results.applied.min_score };
  }

  /** Рисуемый срез: общий или вид запроса, смотря что включено. */
  function compose() {
    const input = queryInput();
    if (input && input.id !== queryOff && queryBuilt?.info.id === input.id) {
      slice = queryBuilt.slice;
      query = queryBuilt.info;
    } else {
      slice = base;
      query = null;
    }
  }

  const queryShape = $derived.by(() => (query ? shapeQuery(slice, query, mode) : null));

  async function requestQuery(fresh = false, checkSources = false) {
    const input = queryInput();
    const path = project();
    if (!input || !path) return;
    const request = queryRequest(input, queryDepth, minS, queryBudget, !checkSources && queryBuilt?.info.id === input.id ? queryBuilt.slice.snapshot : null);
    const key = JSON.stringify({ ...request, snapshot: null });
    const generation = ++queryEpoch;
    if (!checkSources && queryBuilt?.info.id === input.id && key === queryRequestKey) {
      queryLoading = false;
      queryError = "";
      if (fresh) layout?.clear();
      compose();
      rebuild(0.5, fresh);
      return;
    }
    queryLoading = true;
    queryError = "";
    try {
      const server = await loadQuery(path, request);
      if (generation !== queryEpoch || path !== project() || queryInput()?.id !== input.id || queryOff === input.id) return;
      const built = acceptQuerySlice(server, input);
      if (queryBuilt?.info.id === input.id && (queryBuilt.info.changedSnapshot || queryBuilt.slice.snapshot !== built.slice.snapshot)) built.info.changedSnapshot = true;
      const positions = !fresh ? layout?.save() : undefined;
      const selectedId = selected?.type === "node" ? slice.nodes[selected.index]?.id : undefined;
      const selectedPath = selected?.type === "node" ? slice.nodes[selected.index]?.path : undefined;
      layout?.clear();
      queryBuilt = built;
      queryRequestKey = key;
      compose();
      rebuild(fresh ? 0.9 : 0.5, fresh);
      if (positions) layout?.restore(positions);
      selected = selectedId ? { type: "node", index: slice.nodes.findIndex(n => n.id === selectedId || (!!selectedPath && n.path === selectedPath)) } : null;
      if (selected?.type === "node" && selected.index < 0) selected = null;
    } catch (error) {
      if (generation !== queryEpoch || path !== project() || queryInput()?.id !== input.id || queryOff === input.id) return;
      queryError = `Граф запроса не обновлён: ${error instanceof Error ? error.message : String(error)}`;
    } finally {
      if (generation === queryEpoch) queryLoading = false;
    }
  }

  function enterQuery() {
    if (!query && !generalState && layout) generalState = { positions: layout.save(), camera: { ...cam }, w: size.w, h: size.h, selection: selected ? { ...selected } : null, expanded: { ...expanded }, stopped };
    expanded = {};
    packageIndex = app.graphPackage;
    app.graphPackage = null;
    queryOff = "";
    queryDepth = DEFAULT_DEPTH;
    queryBudget = { ...QUERY_BUDGET };
    selected = null;
    void requestQuery(true);
    if (query) announce(`Запрос «${query.text}»: ${query.results.length} результатов, окружение глубины ${queryDepth}`);
  }

  function restoreGeneral() {
    layout?.clear();
    compose();
    if (!generalState) { rebuild(0.6); return; }
    const saved = generalState;
    generalState = null;
    expanded = saved.expanded;
    selected = saved.selection;
    stopped = saved.stopped;
    layout?.setGraph(visibleNodes(), visibleEdges(), owner);
    layout?.restore(saved.positions);
    cam = { ...saved.camera, x: saved.camera.x + (size.w - saved.w) / 2, y: saved.camera.y + (size.h - saved.h) / 2 };
    autoFitSince = 0;
    camTarget = null;
    revision++;
    schedule();
  }

  function leaveQuery() {
    if (!query) return;
    queryOff = query.id;
    queryEpoch++;
    queryError = "";
    queryLoading = false;
    restoreGeneral();
    announce("Общий вид проекта");
  }

  /** Поиск завершился, сброшен или заменён: вид запроса следует за сессией поиска окна. */
  function syncQuery() {
    if (!loaded || loadError) return;
    const input = queryInput();
    if (input) {
      if (input.id !== queryKey) {
        queryKey = input.id;
        enterQuery();
      }
    } else if (!app.searching && !app.searchError && queryKey) {
      queryKey = "";
      queryEpoch++;
      queryBuilt = null;
      queryError = "";
      queryLoading = false;
      queryOff = "";
      if (query) {
        restoreGeneral();
      }
    }
  }

  function setDepth(next: number) {
    if (next === queryDepth) return;
    queryDepth = next;
    queryBudget = { ...QUERY_BUDGET };
    void requestQuery();
  }

  function moreQuery() {
    queryBudget = { nodes: Math.min(QUERY_MAX.nodes, queryBudget.nodes * 2), edges: Math.min(QUERY_MAX.edges, queryBudget.edges * 2) };
    notice = "Показано больше узлов и связей: камера и позиции сохранены.";
    void requestQuery();
  }

  const queryAtMaximum = $derived(queryBudget.nodes >= QUERY_MAX.nodes && queryBudget.edges >= QUERY_MAX.edges);
  const queryOffered = $derived(!query && !!queryOff && queryInput()?.id === queryOff);
  const queryResultCount = $derived(query?.results.filter(r => r.channel !== "related").length ?? 0);
  const queryRelatedCount = $derived(query?.results.filter(r => r.channel === "related").length ?? 0);
  const queryMissing = $derived(query ? query.results.filter((r) => r.status !== "ready" || r.node === null).length : 0);
  const SOURCE_TEXT = { ready: "Актуален", no_graph_object: "Связи для версии выдачи недоступны", changed_since_search: "Документ изменён после поиска", deleted_since_search: "Документ удалён после поиска", unavailable: "Текущий файл недоступен" };
  const SCORE_TEXT: Record<ScoreKind, string> = { rerank: "оценка rerank", cosine: "оценка — косинус (rerank не выполнялся)", none: "оценки нет" };

  function applyForces() {
    if (!layout) return;
    layout.setForces($state.snapshot(forces) as Forces);
    if (reduced) layout.settle(600, true);
    // Мягкий подогрев: карта плавно перетекает к новому равновесию, а не перекладывается заново.
    else if (!stopped) layout.reheat(0.3);
    revision++;
    schedule();
  }

  function resetForces() {
    settings.forces = { ...DEFAULT_FORCES };
    applyForces();
  }

  function onThreshold(event: Event) {
    settings.minS = Number((event.currentTarget as HTMLInputElement).value);
    if (query) { void requestQuery(true); return; }
    rebuild(0.35);
  }

  function visibleEdges(nodes = visibleNodes()): GEdge[] {
    const set = new Set(nodes.map((n) => n.index));
    return slice.edges.filter((e) => kinds[e.kind] && passes(e) && !(mini && e.kind === "chunk_similarity") && set.has(e.a) && set.has(e.b) && (!queryShape?.edgeIds || e.kind === "query_result" || queryShape.edgeIds.has(e.id)));
  }

  /** `fresh` — переложить с нуля (смена режима или первая загрузка): расположение определяют только текущие связи. */
  function rebuild(reheat = 0.55, fresh = false) {
    if (!layout) return;
    const nodes = visibleNodes();
    layout.setGraph(nodes, visibleEdges(nodes), owner);
    if (fresh) {
      layout.reseed();
      // Остановка и reduced motion: раскладка считается сразу, без анимации; иначе — подготовка без показа (первые
      // шаги из рассева самые резкие) и дальше спокойное движение.
      if (reduced || stopped) layout.settle(600, true);
      else {
        layout.settle(100);
        layout.reheat(0.5);
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
      cam = near ? t : { k: cam.k + (t.k - cam.k) * 0.18, x: cam.x + (t.x - cam.x) * 0.18, y: cam.y + (t.y - cam.y) * 0.18 };
      if (near) camTarget = null;
      else schedule();
    }
    paint();
    // Следующий шаг физики — только после показанного кадра: один шаг на кадр.
    layout.frameDone();
    const next = reduced ? "reduced" : stopped ? "stopped" : layout.active && !hidden ? "running" : "idle";
    if (next !== motion) motion = next;
  }

  /** Порция координат от worker: пока раскладка новая и камеру не трогали, граф вписывается по мере роста. */
  function onPositions(_settled: boolean) {
    revision++;
    if (autoFitSince && size.w > 1 && layout?.nodes.length) {
      const now = performance.now();
      const idle = !layout.active || reduced || stopped;
      if (now - autoFitSince > AUTO_FIT_MS) autoFitSince = 0;
      else if (idle || now - lastAutoFit > 300) {
        lastAutoFit = now;
        fit(true);
        if (idle || query) autoFitSince = 0;
      }
    }
    schedule();
  }

  function related(): Set<number> {
    const set = new Set<number>();
    if (!layout) return set;
    if (selected?.type === "node") {
      for (const l of layout.links) {
        if (!kinds[l.edge.kind]) continue;
        if (l.source.index === selected.index) set.add(l.target.index);
        else if (l.target.index === selected.index) set.add(l.source.index);
      }
      // Чанки раскрытого документа и владелец чанка входят в окружение выбора: это группа, а не связь.
      const picked = slice.nodes[selected.index];
      if (picked.kind === "document") for (const c of chunksOfDoc(picked.index).slice(0, expanded[picked.index] ?? 0)) set.add(c);
      else set.add(picked.owner);
    } else if (selected?.type === "edge") {
      const l = layout.links.find((x) => x.edge.id === (selected as { id: number }).id);
      if (l) set.add(l.source.index).add(l.target.index);
    }
    return set;
  }

  function markedNodes(): Set<number> {
    const set = new Set<number>();
    const q = findText.trim().toLowerCase();
    if (!layout) return set;
    for (const n of layout.nodes) {
      if (q && n.node.label.toLowerCase().includes(q)) set.add(n.index);
      if (isolatesOnly && n.degree === 0) set.add(n.index);
    }
    return set;
  }

  /** Цвет модуля для узлов без назначенного цвета: по всем модулям среза, чтобы цвет не менялся от вида. */
  const moduleMap = $derived(moduleColors(base.nodes.filter((n) => n.kind === "document").map((n) => n.module)));

  /** Т-38: цвет документа — свой или папки; чанк берёт цвет документа. Узлы без цвета красятся цветом модуля. */
  const assigned = (node: GNode): string | null => (node.kind === "query" ? null : documentColor(appearance.data, keyOf(node.docsPath)));
  $effect(() => { void appearance.data; untrack(schedule); });

  function packageEdges(): Set<number> {
    const found = new Set<number>();
    const bundle = query?.bundle;
    if (!bundle || packageIndex === null) return found;
    const item = bundle.items.filter(i => ["path", "impact", "relation", "edge"].includes(i.type))[packageIndex];
    if (!item) return found;
    const assertions = item.type === "path" || item.type === "impact" ? item.path.steps.flatMap(step => bundle.assertions.filter(e => e.id === step.edge)) : item.type === "relation" ? [item.assertion] : item.type === "edge" ? [item.edge] : [];
    const ownerPath = (id: string) => {
      const node = bundle.nodes.find(n => n.reference.id === id);
      return bundle.sources.find(s => s.reference.id === id || node?.source_ids.includes(s.reference.id))?.path;
    };
    for (const edge of assertions) {
      const a = ownerPath(edge.from), b = ownerPath(edge.to);
      for (const physical of visibleEdges()) {
        if (physical.kind === "query_result") continue;
        const x = slice.nodes[physical.a], y = slice.nodes[physical.b];
        if (x.ghost || y.ghost) continue;
        const current = [x, y].every(n => bundle.sources.some(s => s.path === n.path && s.reference.revision === n.revision));
        if (current && (x.path === a && y.path === b || x.path === b && y.path === a)) found.add(physical.id);
      }
    }
    return found;
  }

  function paint() {
    if (!canvas || !layout || !palette || size.w < 2) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      rendererError = "Canvas недоступен: карта заменена списком.";
      view = "list";
      return;
    }
    const groups = Object.entries(expanded).map(([o, shown]) => ({ owner: Number(o), shown, total: slice.nodes[Number(o)]?.chunks ?? shown }));
    const scene: Scene = {
      w: size.w, h: size.h, dpr: window.devicePixelRatio || 1, cam, nodes: layout.nodes, links: layout.links, palette, kinds,
      highlighted: packageEdges(), selected, hovered, related: related(), marked: markedNodes(), groups, mini, modules: moduleMap, assigned, focus: isLocal() ? focusIndex : null,
    };
    draw(ctx, scene);
  }

  function fit(auto = false) {
    if (!auto) {
      autoFitSince = 0;
      camTarget = null;
    }
    if (!layout || size.w < 2) return;
    const raw = layout.bounds();
    if (!raw) return;
    // Локальный граф: документ в центре карты, как в Obsidian; границы симметричны вокруг него.
    const centre = isLocal() && focusIndex !== null ? layout.get(focusIndex) : undefined;
    const b = centre
      ? (() => {
          const rx = Math.max(centre.x - raw.x0, raw.x1 - centre.x);
          const ry = Math.max(centre.y - raw.y0, raw.y1 - centre.y);
          return { x0: centre.x - rx, x1: centre.x + rx, y0: centre.y - ry, y1: centre.y + ry };
        })()
      : raw;
    const pad = mini ? 14 : 36;
    const k = Math.min(1.6, Math.max(0.12, Math.min((size.w - pad * 2) / Math.max(40, b.x1 - b.x0), (size.h - pad * 2) / Math.max(40, b.y1 - b.y0))));
    const target = { k, x: size.w / 2 - ((b.x0 + b.x1) / 2) * k, y: size.h / 2 - ((b.y0 + b.y1) / 2) * k };
    if (auto && fitted) camTarget = target;
    else {
      cam = target;
      camTarget = null;
    }
    fitted = true;
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

  async function toggleExpand(index: number) {
    if (mini || query) return;
    const n = slice.nodes[index];
    if (n.kind !== "document" || !n.chunks) return;
    if (expanded[index]) {
      const { [index]: _, ...rest } = expanded;
      expanded = rest;
      if (mode === "chunks" && !Object.keys(rest).length) settings.mode = "meaning";
      if (selected?.type === "node" && slice.nodes[selected.index].owner === index && selected.index !== index) selected = { type: "node", index };
      rebuild(0.5);
    } else {
      expanded = { ...expanded, [index]: Math.min(PORTION, n.chunks) };
      await load("expand");
    }
  }

  async function moreChunks(index: number) {
    const total = slice.nodes[index].chunks;
    expanded = { ...expanded, [index]: Math.min(total, (expanded[index] ?? 0) + PORTION) };
    await load("expand");
  }

  const MODE_NAME: Record<Mode, string> = { links: "Ссылки", meaning: "По смыслу", chunks: "По смыслу · чанки", all: "Все связи" };
  const MODES: Mode[] = ["links", "meaning", "chunks", "all"];
  const modeNote = (m: Mode) =>
    m === "links"
      ? "Режим: ссылки между документами. Стрелка — куда ведёт ссылка"
      : m === "meaning"
        ? `Режим: близость документов по смыслу, сила от ${minS.toFixed(2)}`
        : m === "chunks"
          ? `Режим: близость чанков по смыслу, сила от ${minS.toFixed(2)}`
          : `Режим: все связи — ссылки и близость по смыслу вместе, сила от ${minS.toFixed(2)}`;

  function announce(text: string) {
    toast = text;
    clearTimeout(toastTimer);
    toastTimer = window.setTimeout(() => (toast = ""), 2600);
  }

  const availableModes = $derived<Mode[]>(!mini && shownChunks > 0 ? ["links", "meaning", "chunks", "all"] : ["links", "meaning", "all"]);

  function setMode(next: Mode, say = true) {
    if (next === mode) return;
    settings.mode = next;
    if (selected?.type === "edge" && !kinds[slice.edges[selected.id].kind]) selected = null;
    if (say) announce(modeNote(next));
    rebuild(0.8, true);
  }

  function cycleMode() {
    const list = availableModes;
    setMode(list[(list.indexOf(mode) + 1) % list.length]);
  }

  /** Увеличивает пределы среза до максимума и запрашивает тот же снимок: камера и позиции сохраняются. */
  async function continueSlice() {
    const next = (value: number, max: number) => Math.min(max, value * 2);
    limits = {
      max_documents: next(limits.max_documents, MAX_LIMITS.max_documents),
      max_chunks: next(limits.max_chunks, MAX_LIMITS.max_chunks),
      max_edges: next(limits.max_edges, MAX_LIMITS.max_edges),
      max_bytes: next(limits.max_bytes, MAX_LIMITS.max_bytes),
    };
    notice = "Продолжение того же снимка: добавлены ещё документы и связи, камера и позиции сохранены.";
    await load("more");
  }

  const atMaximum = $derived(slice.applied.max_documents >= MAX_LIMITS.max_documents && slice.applied.max_edges >= MAX_LIMITS.max_edges);

  function select(target: Target) {
    selected = target;
    if (target?.type === "node") notice = "";
    schedule();
  }

  function pointer(event: PointerEvent): [number, number] {
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

  let drag: { mode: "pan" | "node"; index: number; sx: number; sy: number; cam: Camera; moved: boolean; target: Target } | null = null;

  function onDown(event: PointerEvent) {
    if (event.button !== 0 || !canvas) return;
    canvas.setPointerCapture(event.pointerId);
    canvas.focus({ preventScroll: true });
    const [sx, sy] = pointer(event);
    const target = hit(sx, sy);
    drag = target?.type === "node" ? { mode: "node", index: target.index, sx, sy, cam, moved: false, target } : { mode: "pan", index: -1, sx, sy, cam, moved: false, target };
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
      }
      if (drag.mode === "pan") cam = { ...drag.cam, x: drag.cam.x + sx - drag.sx, y: drag.cam.y + sy - drag.sy };
      else {
        const [wx, wy] = toWorld(cam, sx, sy);
        layout?.dragTo(drag.index, wx, wy);
        if (!stopped && !reduced) layout?.reheat(0.25);
      }
      tip = null;
      schedule();
      return;
    }
    const target = hit(sx, sy);
    const same = JSON.stringify(target) === JSON.stringify(hovered);
    if (same) return;
    hovered = target;
    canvas.style.cursor = target ? "pointer" : "grab";
    if (target?.type === "node") tip = { text: `${label(target.index)} · ${slice.nodes[target.index].kind === "document" ? "документ" : slice.nodes[target.index].kind === "query" ? "запрос" : "чанк"}`, x: sx, y: sy };
    else if (target?.type === "edge") {
      const e = slice.edges[target.id];
      tip = { text: `${KIND_TITLE[e.kind]} · ${edgeTitle(e)}`, x: sx, y: sy };
    } else tip = null;
    schedule();
  }

  function onUp() {
    if (!drag) return;
    const d = drag;
    drag = null;
    if (d.mode === "node") layout?.dragEnd(d.index);
    if (!d.moved) select(d.target);
    schedule();
  }

  function onLeave() {
    if (drag) return;
    hovered = null;
    tip = null;
    schedule();
  }

  function onDouble(event: MouseEvent) {
    const r = canvas!.getBoundingClientRect();
    const target = hit(event.clientX - r.left, event.clientY - r.top);
    if (target?.type === "node") void toggleExpand(target.index);
  }

  function onWheel(event: WheelEvent) {
    event.preventDefault();
    const r = canvas!.getBoundingClientRect();
    zoomAt(event.deltaY < 0 ? 1.12 : 1 / 1.12, event.clientX - r.left, event.clientY - r.top);
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

  function onKey(event: KeyboardEvent) {
    const arrows: Record<string, [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };
    if (arrows[event.key]) {
      event.preventDefault();
      if (selected?.type !== "node") {
        const first = [...(layout?.nodes ?? [])].sort((x, y) => y.degree - x.degree)[0];
        if (first) select({ type: "node", index: first.index });
        return;
      }
      const next = neighbourInDirection(selected.index, ...arrows[event.key]);
      if (next !== null) select({ type: "node", index: next });
    } else if (event.key === "Escape") select(null);
    else if (event.key === "Enter" && selected?.type === "node") void toggleExpand(selected.index);
    else if (event.key === " " && selected?.type === "node") {
      event.preventDefault();
      pin(selected.index);
    } else if (event.key.toLowerCase() === "m" || event.key.toLowerCase() === "ь") cycleMode();
    else if (event.key === "+" || event.key === "=") zoomAt(1.25);
    else if (event.key === "-") zoomAt(0.8);
    else if (event.key.toLowerCase() === "f" || event.key === "0") fit();
    else return;
    event.stopPropagation();
  }

  function pin(index: number) {
    const on = layout?.togglePin(index);
    notice = on ? "Узел закреплён: останется на месте при движении и перетаскивании других." : "Закрепление снято.";
    revision++;
    schedule();
  }

  /** Открывает источник: документ целиком, диапазон чанка или строку, где стоит ссылка. */
  function openSource(index: number, line?: number) {
    const n = slice.nodes[index];
    const result = query?.results.find(r => r.node === index);
    if (result?.status === "deleted_since_search" || result?.status === "unavailable") return;
    if (result?.status === "changed_since_search") { void app.openDocument({ path: n.path, title: n.label, first_line: 0, last_line: 0, expectedRevision: n.revision }); return; }
    const first = line ?? (n.kind === "chunk" ? n.firstLine : 0);
    const last = line ?? (n.kind === "chunk" ? n.lastLine : 0);
    void app.openDocument({ path: n.path, title: n.kind === "chunk" ? label(n.owner) : n.label, first_line: first, last_line: last, expectedRevision: slice.nodes[n.owner]?.revision });
  }

  /** Из мини-графа: полный граф вкладкой, в нём — тот же узел выбранным. */
  function openFull(index?: number) {
    if (index !== undefined) app.graphFocus = slice.nodes[index].id;
    app.view = "graph";
  }

  /** «Обновить граф» / «Продолжить» / «Повторить»: запуск построения выбранного слоя. */
  async function startBuild() {
    const path = project();
    if (!path) return;
    try {
      await startGraphBuild(path, buildLayer);
      app.flash("Задание графа принято");
      await refreshStatus();
    } catch (error) {
      app.flash(error instanceof Error ? error.message : String(error), true);
    }
  }

  /** «Остановить построение»: останавливает только идущее задание, автообновление будущих изменений не выключается (OQ-190). */
  async function stopBuild() {
    const path = project();
    const job = status?.active_job;
    if (!path || !job) return;
    try {
      await stopGraphBuild(path, job);
      app.flash("Остановка запрошена; исход появится в состоянии графа");
      await refreshStatus();
    } catch (error) {
      app.flash(error instanceof Error ? error.message : String(error), true);
    }
  }

  function runFind(event: KeyboardEvent) {
    if (event.key !== "Enter") return;
    const q = findText.trim().toLowerCase();
    const found = layout?.nodes.filter((n) => n.node.label.toLowerCase().includes(q)) ?? [];
    if (found.length) select({ type: "node", index: found[0].index });
  }

  // Загрузка среза. Каждый ответ полный для области, пределов и раскрытых документов; ответ чужого проекта отбрасывается.
  type Reason = "open" | "expand" | "more" | "refresh";

  async function load(reason: Reason) {
    const path = project();
    if (!path || !layout) return;
    const id = ++epoch;
    loading = true;
    const request = {
      scope: $state.snapshot(app.scope),
      limits,
      expanded: Object.entries(expanded).map(([index, shown]) => ({ document: store.nodes[Number(index)].id, shown })),
      snapshot: reason === "open" || reason === "refresh" ? null : (slice.snapshot || null),
    };
    try {
      const server = await loadPhysical(path, request);
      if (id !== epoch || path !== project()) return;
      base = store.apply(server);
      compose();
      loaded = true;
      loadError = "";
      loadErrorKind = "";
      // Раскрытый документ мог исчезнуть из области или потерять чанки: раскрытие без узлов не держим.
      const alive = Object.fromEntries(Object.entries(expanded).filter(([index]) => chunksOfDoc(Number(index)).length > 0 && slice.visibleDocs.has(Number(index))));
      if (Object.keys(alive).length !== Object.keys(expanded).length) expanded = alive;
      const picked = selected;
      if (picked?.type === "node" && !visibleNodes().some((n) => n.index === picked.index)) selected = null;
      else if (picked?.type === "edge") selected = null;
      rebuild(reason === "open" ? 0.9 : 0.45, reason === "open");
    } catch (error) {
      if (id !== epoch || path !== project()) return;
      if (error instanceof BarsError && error.kind === "snapshot_expired") {
        loading = false;
        await load("refresh");
        return;
      }
      loadError = error instanceof Error ? error.message : String(error);
      loadErrorKind = error instanceof BarsError ? (error.kind ?? "") : "";
      if (reason === "open" || reason === "refresh") {
        base = emptySlice();
        slice = base;
        query = null;
        loaded = true;
        rebuild(0.5, true);
      }
    } finally {
      if (id === epoch) loading = false;
    }
  }

  async function refreshStatus() {
    const path = project();
    if (!path) return;
    try {
      const value = await graphStatus(path);
      if (path !== project()) return;
      status = value;
      const ready = !!value.snapshot;
      const changed = !!value.snapshot && value.snapshot !== slice.snapshot;
      if (query && changed && !queryLoading) void requestQuery(false, true);
      if (!query && ready && !loading && (changed || (loadError && loadErrorKind === "graph_not_ready"))) void load(slice.snapshot ? "refresh" : "open");
    } catch {
      // Состояние графа не прочиталось: срез и его ошибка остаются на экране.
    }
  }

  function resetFor(path: string | null) {
    epoch++;
    queryEpoch++;
    queryBuilt = null;
    generalState = null;
    packageIndex = null;
    queryError = "";
    queryLoading = false;
    layout?.destroy();
    layout = new Layout();
    layout.paced = true;
    layout.onPositions = onPositions;
    layout.setForces($state.snapshot(settings.forces) as Forces);
    store = new SliceStore();
    base = emptySlice();
    slice = base;
    query = null;
    queryKey = "";
    queryOff = "";
    expanded = {};
    selected = null;
    loaded = false;
    openedOnObject = false;
    loading = false;
    loadError = "";
    loadErrorKind = "";
    status = null;
    fitted = false;
    stopped = false;
    limits = { ...INITIAL_LIMITS };
    if (!path) return;
    void load("open");
    void refreshStatus();
  }

  // Данные для разметки.
  const selectedNode = $derived(selected?.type === "node" ? slice.nodes[selected.index] : null);
  const selectedEdge = $derived(selected?.type === "edge" ? slice.edges[selected.id] : null);
  const connections = $derived.by(() => {
    void revision;
    const sel = selected;
    if (sel?.type !== "node" || !layout) return [] as { edge: GEdge; other: number }[];
    return layout.links
      .filter((l) => kinds[l.edge.kind] && (l.source.index === sel.index || l.target.index === sel.index))
      .map((l) => ({ edge: l.edge, other: l.source.index === sel.index ? l.target.index : l.source.index }))
      .sort((a, b) => b.edge.s - a.edge.s);
  });
  const springOfSelected = $derived.by(() => {
    void revision;
    return selected?.type === "edge" ? layout?.links.find((l) => l.edge.id === (selected as { id: number }).id)?.k ?? null : null;
  });
  const counts = $derived.by(() => {
    void revision;
    const result: Record<EdgeKind, number> = { markdown_link: 0, document_similarity: 0, chunk_similarity: 0, query_result: 0 };
    const nodes = new Set(visibleNodes().map((n) => n.index));
    for (const e of slice.edges) if (nodes.has(e.a) && nodes.has(e.b) && !(mini && e.kind === "chunk_similarity") && passes(e)) result[e.kind]++;
    return result;
  });
  const totalCounts = $derived.by(() => {
    const result: Record<EdgeKind, number> = { markdown_link: 0, document_similarity: 0, chunk_similarity: 0, query_result: 0 };
    const nodes = new Set(visibleNodes().map((n) => n.index));
    for (const e of slice.edges) if (nodes.has(e.a) && nodes.has(e.b)) result[e.kind]++;
    return result;
  });
  const pinnedSel = $derived.by(() => {
    void revision;
    return selectedNode ? (layout?.get(selectedNode.index)?.pinned ?? false) : false;
  });
  const isolates = $derived.by(() => {
    void revision;
    return layout?.nodes.filter((n) => n.degree === 0).length ?? 0;
  });
  const rows = $derived.by(() => {
    void revision;
    const q = listFilter.trim().toLowerCase();
    return (layout?.nodes ?? visibleNodes().map((node) => ({ index: node.index, node, degree: 0 }) as PNode))
      .filter((n) => (!q || n.node.label.toLowerCase().includes(q) || (n.node.path ?? "").toLowerCase().includes(q)) && (!isolatesOnly || n.degree === 0))
      .sort((a, b) => b.degree - a.degree || a.node.label.localeCompare(b.node.label));
  });
  const layerIssues = $derived(loaded && !loadError ? EDGE_KINDS.filter((k) => slice.layers[k].state !== "ready" && (k !== "chunk_similarity" || shownChunks > 0)) : []);
  const STATE_TEXT: Record<string, string> = { ready: "готов", building: "строится", partial: "частично", stale: "устарел", failed: "ошибка", cancelled: "отменён", not_built: "не построен", disabled: "выключен" };
  const MOTION_TEXT = { running: "Движение идёт", idle: "Раскладка в покое", stopped: "Остановлено вами", reduced: "Без движения (reduced motion)" };

  const sampleS = [0.5, 0.65, 0.8, 0.95];
  const dash = (kind: EdgeKind) => (kind === "chunk_similarity" ? "5 3" : "0");
  const edgeLine = (e: GEdge) => ({ w: widthOf(drawStrength(e)), b: brightnessOf(drawStrength(e)) });
  const reverseLink = (e: GEdge) => (e.kind === "markdown_link" ? slice.edges.find((x) => x.kind === "markdown_link" && x.a === e.b && x.b === e.a) : undefined);
  const edgeTitle = (e: GEdge) => {
    if (e.kind !== "markdown_link") return `${label(e.a)} — ${label(e.b)}`;
    return reverseLink(e) ? `${label(e.a)} и ${label(e.b)} ссылаются друг на друга` : `${label(e.a)} → ${label(e.b)}`;
  };
  const linesText = (e: GEdge) => (e.lines.length ? `строки ${e.lines.slice(0, 8).join(", ")}${e.lines.length > 8 ? "…" : ""}` : "строки неизвестны");
  const CONN_KINDS: EdgeKind[] = [...EDGE_KINDS, "query_result"];
  const originText = (e: GEdge) =>
    e.kind === "query_result"
      ? "Связь запроса с результатом поиска: сила — оценка поиска, а не связь между документами; в Markdown и индекс ничего не записывается."
      : e.kind === "markdown_link"
      ? reverseLink(e)
        ? `Взаимные явные ссылки: «${label(e.a)}» → «${label(e.b)}» (${linesText(e)}) и обратная «${label(e.b)}» → «${label(e.a)}» (${linesText(reverseLink(e)!)}). На карте одна линия со стрелкой на каждом конце; направления хранятся отдельно.`
        : `Явная Markdown-ссылка из «${label(e.a)}» в «${label(e.b)}» (${linesText(e)}); стрелка на конце, куда ведёт ссылка. Число упоминаний не усиливает пружину.`
      : e.kind === "document_similarity"
        ? query
          ? "Метод document-centroid-v1: точный косинус готовых документных представлений внутри области поиска; общий kNN не используется."
          : "Метод document-centroid-v1: косинус документных представлений — взвешенного среднего готовых векторов чанков документа. Пара попала в k=5 ближайших хотя бы одного из двух документов при cosine не ниже 0,5."
        : slice.nodes[e.a].owner === slice.nodes[e.b].owner
          ? "Косинус готовых векторов двух чанков одного документа: общий владелец, не новое межфайловое знание."
          : "Косинус готовых векторов двух чанков; kNN k=5, tau=0.5, неориентированная пара.";
  const modeCount = (m: Mode) => modeKinds(m).reduce((sum, k) => sum + counts[k], 0);
  const modeTotal = (m: Mode) => modeKinds(m).reduce((sum, k) => sum + totalCounts[k], 0);
  const modeLayer = (m: Mode) => modeKinds(m).map((k) => ({ kind: k, layer: slice.layers[k] })).find((x) => x.layer.state !== "ready" && (x.kind !== "chunk_similarity" || shownChunks > 0));
  const pct = (v: number) => `${Math.round(v * 100)}%`;
  const nodeCount = $derived.by(() => {
    void revision;
    return layout?.nodes.length ?? 0;
  });
  const linkCount = $derived.by(() => {
    void revision;
    return layout?.links.length ?? 0;
  });
  const graphNotReady = $derived(loadErrorKind === "graph_not_ready");
  const controls = $derived(buildControls(status));
  const building = $derived(controls.building);
  let buildLayer = $state("all");
  const selectedKind = $derived(selectedNode ? selectedNode.kind : null);

  onMount(() => {
    mini = !!host.closest(".mini");
    palette = readPalette(host);
    settings = project() ? loadSettings(project()!, panelKind()) : defaultSettings();
    mounted = true;
    hostObserver = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(([entry]) => {
      const w = Math.floor(entry.contentRect.width);
      const h = Math.floor(entry.contentRect.height);
      // Размер панели меняет окно просмотра, но не вписывает граф заново.
      if (fitted) cam = { ...cam, x: cam.x + (w - size.w) / 2, y: cam.y + (h - size.h) / 2 };
      size = { w, h };
      if (canvas) {
        const dpr = window.devicePixelRatio || 1;
        canvas.width = Math.max(1, Math.round(w * dpr));
        canvas.height = Math.max(1, Math.round(h * dpr));
      }
      if (!fitted && w > 1 && h > 1 && layout?.nodes.length) {
        fit(true);
      }
      schedule();
    });
    hostObserver?.observe(stage);
    themeObserver = new MutationObserver(() => {
      palette = readPalette(host);
      schedule();
    });
    themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["data-mode"] });
    io = typeof IntersectionObserver === "undefined" ? null : new IntersectionObserver(([entry]) => {
      hidden = !entry.isIntersecting || document.hidden;
      layout?.hold(hidden);
      if (!hidden) schedule();
    });
    io?.observe(host);
    document.addEventListener("visibilitychange", onVisibility);
    statusTimer = setInterval(() => void refreshStatus(), 2000);
  });

  function onVisibility() {
    hidden = document.hidden;
    layout?.hold(hidden);
    if (!hidden) schedule();
  }

  onDestroy(() => {
    epoch++;
    queryEpoch++;
    if (raf) cancelAnimationFrame(raf);
    if (statusTimer) clearInterval(statusTimer);
    hostObserver?.disconnect();
    themeObserver?.disconnect();
    io?.disconnect();
    document.removeEventListener("visibilitychange", onVisibility);
    layout?.destroy();
    layout = null;
  });

  // Смена проекта или области: новая модель, позиции, камера и раскрытия прежнего среза не переносятся.
  $effect(() => {
    const path = projectPath;
    void scopeKey;
    if (!mounted) return;
    untrack(() => {
      if (path) settings = loadSettings(path, panelKind());
      resetFor(path);
    });
  });
  $effect(() => {
    const snapshot = $state.snapshot(settings);
    const path = projectPath;
    if (mounted && path) untrack(() => saveSettings(path, panelKind(), snapshot));
  });
  // Из мини-графа: «В графе» выбирает узел в полном графе.
  $effect(() => {
    const id = app.graphFocus;
    void query;
    void revision;
    if (!id || mini || !loaded || app.graphShowQuery || queryLoading) return;
    untrack(() => {
      const index = slice.nodes.find(n => n.id === id || n.path === id)?.index;
      if (index === undefined || !layout?.get(index)) return;
      app.graphFocus = null;
      selected = { type: "node", index };
      const p = layout.get(index)!;
      autoFitSince = 0;
      camTarget = null;
      fitted = true;
      if (size.w > 1) cam = { k: Math.max(cam.k, 1.1), x: size.w / 2 - p.x * Math.max(cam.k, 1.1), y: size.h / 2 - p.y * Math.max(cam.k, 1.1) };
      schedule();
    });
  });
  // Открытие вкладки на объекте: если есть открытый документ и нет ни завершённого поиска, ни перехода из мини-графа,
  // полный граф один раз выбирает этот документ и центрирует камеру. Вид запроса и «Общий вид» остаются прежними (OQ-204).
  $effect(() => {
    void revision;
    if (mini || !loaded || loadError || openedOnObject || loading || queryLoading) return;
    untrack(() => {
      if (app.graphFocus || app.graphShowQuery || queryInput() || query) { openedOnObject = true; return; }
      const path = app.activeDocument;
      const index = path ? base.nodes.find((n) => n.kind === "document" && n.path === path && base.visibleDocs.has(n.index))?.index : undefined;
      if (index !== undefined && !layout?.get(index)) return;
      openedOnObject = true;
      if (index === undefined) return;
      selected = { type: "node", index };
      const p = layout!.get(index)!;
      const k = Math.max(cam.k, 1.1);
      autoFitSince = 0;
      camTarget = null;
      fitted = true;
      if (size.w > 1) cam = { k, x: size.w / 2 - p.x * k, y: size.h / 2 - p.y * k };
      schedule();
    });
  });
  // Мини-граф: другой активный документ или другая глубина — локальный граф раскладывается с нуля и вписывается.
  $effect(() => {
    void focusIndex;
    void settings.depth;
    if (!mounted || !mini) return;
    untrack(() => {
      if (!loaded || loadError || query) return;
      selected = null;
      rebuild(0.9, true);
    });
  });
  // Вид запроса следует за сессией поиска окна: включается после каждого завершённого поиска и выключается, когда поиск сброшен.
  $effect(() => {
    void app.results;
    void app.displayed;
    void app.searching;
    void app.searchError;
    void loaded;
    void loadError;
    if (!mounted) return;
    untrack(syncQuery);
  });
  // «Открыть связи и основания» из выдачи: вид запроса включается снова, даже если из него вышли в общий вид.
  $effect(() => {
    if (!app.graphShowQuery || mini || !loaded || loadError) return;
    untrack(() => {
      app.graphShowQuery = false;
      const input = queryInput();
      if (!input) return;
      queryKey = input.id;
      if (!query || queryOff) enterQuery();
      else { packageIndex = app.graphPackage; app.graphPackage = null; schedule(); }
    });
  });
  $effect(() => {
    void selected;
    void findText;
    void isolatesOnly;
    schedule();
  });
</script>

{#snippet forcesForm()}
  {#if mini}
    <h4>Локальный граф</h4>
    <label class="opt slider" title="Соседи открытого документа (1) или и соседи соседей (2)"><span>Глубина</span><input type="range" min="1" max="2" step="1" aria-label="Глубина локального графа" bind:value={settings.depth} /><b>{settings.depth}</b></label>
  {/if}
  {#if mode !== "links"}
    <h4>Связи по смыслу</h4>
    <label class="opt thr-row" title="Показывать близость не слабее выбранной силы s. Сила остальных рёбер не меняется.">
      <span>Сила от</span><input type="range" min="0.5" max="0.95" step="0.01" aria-label="Порог показа близости" bind:value={settings.minS} oninput={onThreshold} /><b>{minS.toFixed(2)}</b>
    </label>
  {/if}
  <h4>Раскладка</h4>
  <label class="opt slider" title="Насколько сильно связи сближают узлы"><span>Притяжение связей</span><input type="range" min={FORCE_RANGE.link.min} max={FORCE_RANGE.link.max} step={FORCE_RANGE.link.step} aria-label="Притяжение по связям" bind:value={settings.forces.link} oninput={applyForces} /><b>×{forces.link.toFixed(2)}</b></label>
  <label class="opt slider" title="Насколько узлы расталкивают друг друга"><span>Отталкивание</span><input type="range" min={FORCE_RANGE.repel.min} max={FORCE_RANGE.repel.max} step={FORCE_RANGE.repel.step} aria-label="Отталкивание узлов" bind:value={settings.forces.repel} oninput={applyForces} /><b>×{forces.repel.toFixed(2)}</b></label>
  <label class="opt slider" title="Все узлы слегка тянутся к общему центру; документы без связей стоят кольцом вокруг карты"><span>Общее притяжение</span><input type="range" min={FORCE_RANGE.gravity.min} max={FORCE_RANGE.gravity.max} step={FORCE_RANGE.gravity.step} aria-label="Общее притяжение узлов" bind:value={settings.forces.gravity} oninput={applyForces} /><b>×{forces.gravity.toFixed(2)}</b></label>
  <label class="opt slider" title="Расстояние между связанными документами: меняет масштаб всей карты"><span>Длина связи</span><input type="range" min={FORCE_RANGE.length.min} max={FORCE_RANGE.length.max} step={FORCE_RANGE.length.step} aria-label="Длина связи" bind:value={settings.forces.length} oninput={applyForces} /><b>{forces.length}</b></label>
  <button class="reset" onclick={resetForces}>По умолчанию</button>
  <p class="hint">Меняет движение узлов, а не силу связи: толщина, яркость и значения s прежние.</p>
  {#if !mini}
    <h4>Обновление графа</h4>
    <label class="opt" title="Какой слой перестраивает кнопка «Обновить граф»"><span>Слой</span><select aria-label="Слой для обновления" bind:value={buildLayer} disabled={building}><option value="all">Все слои</option><option value="structure">Структура</option><option value="semantic">Отношения</option><option value="similarity">Близость</option></select></label>
    <button class="reset" aria-expanded={legendOpen} onclick={() => { legendOpen = !legendOpen; popover = null; }}>Легенда и шкала силы</button>
    <details class="facts">
      <summary>Сведения</summary>
      <table aria-label="Сведения о графе">
        <tbody>
          <tr><th>Документов</th><td>{slice.docCount} из {slice.totals.documents}</td></tr>
          <tr><th>Чанков раскрыто</th><td>{shownChunks} из {slice.totals.chunks}</td></tr>
          <tr><th>Снимок графа</th><td title={slice.snapshot}>{slice.snapshot.slice(0, 8) || "—"}</td></tr>
          {#each EDGE_KINDS as kind (kind)}<tr><th>{KIND_TITLE[kind]}</th><td>{kind === "chunk_similarity" && !shownChunks ? "при раскрытии" : counts[kind]}{totalCounts[kind] > counts[kind] ? ` из ${totalCounts[kind]}` : ""} · слой {STATE_TEXT[slice.layers[kind].state]}</td></tr>{/each}
        </tbody>
      </table>
    </details>
  {/if}
{/snippet}

<div class="pgw" bind:this={host}>
{#if queryLoading}<p role="status">Строится граф запроса…</p>{/if}
{#if queryError}<p role="alert">{queryError}</p>{/if}
<div class="pg" class:mini role="region" aria-label={mini ? "Мини-граф" : "Физический граф проекта"} data-mode={mode} data-motion={motion} data-nodes={nodeCount} data-links={linkCount} data-loaded={loaded}>
  {#if !mini}
    <div class="top">
    <div class="bar" role="toolbar" aria-label="Управление графом">
      <label class="find">
        <Icon name="search" size={13} width={1.8} />
        <input type="search" placeholder="Найти узел" aria-label="Найти узел графа" bind:value={findText} onkeydown={runFind} />
        {#if findText.trim()}<span class="found" role="status">{markedNodes().size}</span>{/if}
      </label>
      <div class="modes" role="radiogroup" aria-label="Режим связей">
        {#each MODES as m (m)}
          {@const issue = modeLayer(m)}
          {@const on = modeKinds(m).filter((k) => k !== "chunk_similarity" || m === "chunks" || shownChunks > 0)}
          <button class="mode" role="radio" aria-checked={mode === m} disabled={m === "chunks" && !shownChunks} title={m === "chunks" && !shownChunks ? "Раскройте чанки документа, чтобы увидеть близость чанков" : `${issue ? issue.layer.note : MODE_NAME[m]}. Клавиша M — следующий режим`} onclick={() => setMode(m)}>
            <svg width="22" height="12" viewBox="0 0 22 12" aria-hidden="true">
              {#each on as kind, i (kind)}
                <line x1="1" y1={on.length === 1 ? 6 : 3 + i * 3} x2="18" y2={on.length === 1 ? 6 : 3 + i * 3} stroke={palette?.kindCss[kind] ?? "currentColor"} stroke-width="2" stroke-dasharray={dash(kind)} />
              {/each}
              {#if m === "links"}<path d="M21 6l-5-3v6z" fill={palette?.kindCss.markdown_link ?? "currentColor"} />{/if}
            </svg>
            {MODE_NAME[m]}
            <b>{m === "chunks" && !shownChunks ? "—" : modeCount(m)}</b>
            {#if issue}<span class="state {issue.layer.state}">{STATE_TEXT[issue.layer.state]}</span>{/if}
          </button>
        {/each}
      </div>
      <span class="sep"></span>
      <div class="tools">
        <button aria-label="Уменьшить масштаб" title="Уменьшить масштаб" onclick={() => zoomAt(0.8)}><Icon name="minus" size={14} width={1.8} /></button>
        <button aria-label="Увеличить масштаб" title="Увеличить масштаб" onclick={() => zoomAt(1.25)}><Icon name="plus" size={14} width={1.8} /></button>
        <button aria-label="Вписать" title="Вписать граф в панель (F)" onclick={() => fit()}><Icon name="fit" size={14} width={1.8} /></button>
        <button aria-label={stopped ? "Запустить движение" : "Остановить движение"} aria-pressed={stopped} title={stopped ? "Запустить движение" : "Остановить движение"} onclick={() => setStopped(!stopped)} disabled={reduced}><Icon name={stopped ? "play" : "pause"} size={14} width={1.8} /></button>
        <button aria-label={view === "list" ? "Показать карту" : "Показать списком"} aria-pressed={view === "list"} title={view === "list" ? "Показать карту" : "Показать списком"} onclick={() => (view = view === "list" ? "map" : "list")}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01" /></svg>
        </button>
        <button aria-label="Настройки графа" aria-expanded={popover === "settings"} title="Настройки графа: раскладка, порог, слои, легенда, сведения" onclick={() => (popover = popover === "settings" ? null : "settings")}><Icon name="gear" size={14} width={1.8} /></button>
        <button class="more-menu" aria-label="Ещё" title="Ещё" aria-haspopup="menu" aria-expanded={popover === "more"} onclick={() => (popover = popover === "more" ? null : "more")}>⋯</button>
      </div>
    </div>
    <div class="entry-actions" role="group" aria-label="Действия над документом">
      {#if selectedNode}
        <b class="entry-title" title={selectedNode.label}>{selectedNode.label}</b>
        {#each GRAPH_ACTIONS as action (action.id)}
          {@const state = actionState(action, selectedKind)}
          <button class="act" data-action={action.id} disabled={!state.enabled} title={state.reason}>{action.label}</button>
        {/each}
      {:else}
        <span class="entry-hint">{NO_SELECTION_TEXT}</span>
        {#each GRAPH_ACTIONS as action (action.id)}
          <button class="act" data-action={action.id} disabled title={actionState(action, null).reason}>{action.label}</button>
        {/each}
      {/if}
    </div>
    {#if query}
      <div class="qstack">
      <div class="qbar" role="region" aria-label="Вид запроса">
        <span class="qsq" aria-hidden="true"></span>
        <b class="qtext" title={query.text}>Запрос «{query.text}»</b>
        <span>{queryResultCount} результатов{#if queryRelatedCount} · {queryRelatedCount} связанных источников{/if}</span>
        <button class="more" disabled={queryLoading} onclick={() => void requestQuery(false, true)}>Проверить источники</button>
        <span>{SEARCH_MODE[query.applied]}{query.requested !== query.applied ? ` (запрошен «${SEARCH_MODE[query.requested]}»)` : ""} · {SCORE_TEXT[query.scoreKind]}</span>
        <span class="depth" role="radiogroup" aria-label="Окружение результатов">Окружение
          {#each Array.from({ length: MAX_DEPTH + 1 }, (_, i) => i) as d (d)}<button role="radio" aria-checked={queryDepth === d} class:on={queryDepth === d} onclick={() => setDepth(d)}>{d}</button>{/each}
        </span>
        <button class="more" onclick={leaveQuery}>Общий вид</button>
      </div>
      {#if query.fallback || queryMissing || query.changedSnapshot || !query.results.length}
        <div class="banner" role="status">
          {#if query.fallback}<span><b>Режим поиска изменён:</b> {fallbackText(query.fallback)}</span>{/if}
          {#if query.changedSnapshot}<span>Граф обновлён после поиска; выдача и её оценки сохранены.</span>{/if}
          {#if queryMissing}<span><b>Граф неполон:</b> {queryMissing} источников изменены, недоступны или вне карты — причины в инспекторе запроса.</span>{/if}
          {#if !query.results.length}<span><b>Ничего не найдено:</b> порог {query.minScore.toFixed(2)}.</span>{/if}
        </div>
      {/if}
      {#if queryShape?.truncated}
        <div class="banner" role="status">
          <span><b>Показано {queryShape.shown.nodes} из {queryShape.known.nodes} узлов · {queryShape.shown.edges} из {queryShape.known.edges} связей.</b> Результаты не вытесняются окружением.</span>
          {#if queryAtMaximum}<span>Достигнут предел вида: уменьшите глубину или режим связей.</span>{:else}<button class="more" onclick={moreQuery}>Показать ещё</button>{/if}
        </div>
      {/if}
      </div>
    {/if}
    {#if status}
      <div class="banner state" role="status" aria-label="Состояние графа">
        <span>{graphDataLabel(status)} · {completenessLabels[status.completeness ?? "unknown"]}{jobLabels[status.job_state ?? "idle"] ? ` · ${jobLabels[status.job_state ?? "idle"]}` : ""}</span>
        {#if controls.staleReason}<span>Ждёт обновления: {controls.staleReason}</span>{/if}
        <button class="more" disabled={!controls.updateEnabled} title={controls.updateReason} onclick={() => void startBuild()}>{controls.updateLabel}</button>
        <button class="more ghost" disabled={!controls.stopEnabled} title={controls.stopReason} onclick={() => void stopBuild()}>Остановить построение</button>
        {#if building && status.progress}<span>{({ structure: "Разбор документов", similarity: "Расчёт близости", similarity_pairs: "Сравнено пар", publish: "Публикация" } as Record<string, string>)[status.progress.phase] ?? "Построение"}: {status.progress.processed}{status.progress.total === null ? "" : ` / ${status.progress.total}`} · {(status.progress.elapsed_ms / 1000).toFixed(1)} с</span>{/if}
        {#if status.file_errors_total}<details><summary>Файлов с ошибками: {status.file_errors_total}</summary>{#each status.file_errors ?? [] as error}<div>{error.path}: {error.message}</div>{/each}</details>{/if}
        {#if status.error}<span>{status.error}</span>{/if}
      </div>
    {/if}
    {#if layerIssues.length}
      <div class="banner" role="status">
        {#each layerIssues as kind (kind)}<span><b>{KIND_TITLE[kind]}:</b> {slice.layers[kind].note}.</span>{/each}
      </div>
    {/if}
    {#if loaded && !loadError && slice.truncated}
      <div class="banner" role="status">
        <span><b>Граф усечён:</b> {slice.limitReasons.join("; ")}. Связи между показанными узлами полны; остальное не потеряно.</span>
        {#if atMaximum}<span>Достигнут максимум среза: сузьте область в дереве.</span>{:else}<button class="more" onclick={() => void continueSlice()} disabled={loading}>Показать ещё</button>{/if}
      </div>
    {/if}
    </div>
  {/if}

  <div class="stage" bind:this={stage}>
    {#if view === "map"}
      {#if rendererError}
        <p class="empty" role="alert">{rendererError}</p>
      {:else if !loaded}
        <p class="empty" role="status">Загрузка графа…</p>
      {:else if loadError}
        <div class="empty" role="alert">
          <b>{graphNotReady ? (building ? "Граф строится" : "Граф ещё не построен или устарел") : "Граф недоступен"}</b>
          <span>{loadError}</span>
          {#if graphNotReady && !building}<button class="act" disabled={!controls.updateEnabled} title={controls.updateReason} onclick={() => void startBuild()}>{controls.updateLabel}</button>{/if}
        </div>
      {:else if slice.docCount === 0}
        <div class="empty" role="status"><b>В выбранной области нет документов</b><span>Измените область в дереве или в фильтре документов.</span></div>
      {/if}
      <canvas
        bind:this={canvas}
        class:hide={slice.docCount === 0 || !!loadError || !loaded}
        tabindex="0"
        aria-label="Карта графа. Стрелки — выбор соседнего узла, M — режим связей, Enter — раскрыть чанки, пробел — закрепить, F — вписать, Escape — снять выбор"
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
      {#if loaded && !loadError && slice.docCount > 0 && slice.edges.length === 0}<p class="note" role="status">У документов области нет допущенных связей: порог близости не пройден, ссылок нет.</p>{/if}
      {#if toast}<div class="toast" role="status" aria-live="polite">{toast}</div>{/if}
      <div class="status" role="status">
        <span class="dot {motion}" title={MOTION_TEXT[motion]}></span>
        {#if query}
          {#if !mini}{MOTION_TEXT[motion]} · {/if}<b>Запрос: {queryResultCount} результатов</b>{#if queryRelatedCount} · {queryRelatedCount} связанных источников{/if} · узлов {queryShape?.shown.nodes ?? 0}{#if !mini} · связей {queryShape?.shown.edges ?? 0}{/if} · {MODE_NAME[mode].toLowerCase()}
        {:else if mini && isLocal() && focusIndex !== null}
          <b>Локальный граф</b> · <span class="lt" title={slice.nodes[focusIndex].label}>{slice.nodes[focusIndex].label}</span>{#if nodeCount <= 1} · нет связей в режиме «{MODE_NAME[mode]}»{:else} · {MODE_NAME[mode].toLowerCase()}{/if}
        {:else if mini}
          <b>Документы {slice.docCount}</b> · {MODE_NAME[mode].toLowerCase()}
        {:else}
          {MOTION_TEXT[motion]} · <b>Документы {slice.docCount} из {slice.totals.documents}</b>{#if shownChunks} · чанков {shownChunks} из {slice.totals.chunks}{/if}{#if isolates} · <button class="inline" onclick={() => (isolatesOnly = !isolatesOnly)} aria-pressed={isolatesOnly}>без связей: {isolates}</button>{/if}
        {/if}
      </div>
      {#if mini && query}
        <div class="qplate"><span class="qsq" aria-hidden="true"></span><span class="qt" title={query.text}>Запрос «{query.text}»</span><button class="inline" onclick={leaveQuery}>Общий вид</button></div>
      {/if}
      {#if queryOffered}
        <div class="qplate dashed"><span class="qt" title={queryInput()?.text}>Запрос «{queryInput()?.text}»</span><button class="inline" onclick={enterQuery}>{mini ? "Показать" : "Показать запрос в графе"}</button></div>
      {/if}
      {#if mini}
        <div class="mini-tools">
          <button aria-label="Вписать" title="Вписать" onclick={() => fit()}><Icon name="fit" size={13} width={1.8} /></button>
          <button aria-label={stopped ? "Запустить движение" : "Остановить движение"} title={stopped ? "Запустить движение" : "Остановить движение"} onclick={() => setStopped(!stopped)} disabled={reduced}><Icon name={stopped ? "play" : "pause"} size={13} width={1.8} /></button>
          <button class="mode-btn" aria-label="Режим связей: {MODE_NAME[mode]}. Переключить" title="Связи: {MODE_NAME[mode]} (клик — следующий режим)" onclick={cycleMode}>
            {#if mode === "links"}
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M7 17 17 7M9 7h8v8" /></svg>
            {:else if mode === "all"}
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3 3 8l9 5 9-5-9-5zM3 13l9 5 9-5" /></svg>
            {:else}
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M4 9c3-3 5 3 8 0s5 3 8 0M4 16c3-3 5 3 8 0s5 3 8 0" /></svg>
            {/if}
          </button>
          <button aria-label="Настройки раскладки" aria-expanded={popover === "settings"} title="Настройки раскладки" onclick={() => (popover = popover === "settings" ? null : "settings")}><Icon name="gear" size={13} width={1.8} /></button>
          <button aria-label="Открыть полный граф" title="Открыть полный граф вкладкой" onclick={() => openFull()}><Icon name="arrow-right" size={13} width={1.8} /></button>
        </div>
        {#if popover === "settings"}
          <section class="pop" aria-label="Настройки раскладки">{@render forcesForm()}</section>
        {/if}
        {#if selectedNode}
          <div class="caption" role="status">
            <b title={selectedNode.label}>{selectedNode.label}</b>
            <span>{connections.length} связей</span>
            {#if selectedNode.kind !== "query"}
              <button class="inline" onclick={() => openSource(selectedNode.index)}>Открыть</button>
              <button class="inline" onclick={() => openFull(selectedNode.index)}>В графе</button>
            {/if}
          </div>
        {/if}
      {/if}
      {#if popover === "settings" && !mini}
        <section class="pop full" aria-label="Настройки графа">{@render forcesForm()}</section>
      {/if}
      {#if popover === "more" && !mini}
        <div class="pop menu" role="menu" aria-label="Ещё" tabindex="-1" onkeydown={(e) => { if (e.key === "Escape") { e.stopPropagation(); popover = null; } }}>
          <button role="menuitem" onclick={() => { popover = null; app.graphAnalysis = true; }}>Анализ связей<small>прежний экран: пути, влияние, контекст</small></button>
        </div>
      {/if}
      {#if legendOpen && !mini}
        <section class="legend" aria-label="Легенда графа">
          <h4>Виды связей</h4>
          {#each EDGE_KINDS as kind (kind)}
            <div class="row"><svg width="34" height="12" viewBox="0 0 34 12" aria-hidden="true"><line x1="1" y1="6" x2="29" y2="6" stroke={palette?.kindCss[kind] ?? "currentColor"} stroke-width="2.2" stroke-dasharray={dash(kind)} />{#if kind === "markdown_link"}<path d="M33 6l-6-3.5v7z" fill={palette?.kindCss[kind] ?? "currentColor"} />{/if}</svg>{KIND_TITLE[kind]}<small>{kind === "markdown_link" ? "стрелка на конце «куда ссылаются»" : kind === "chunk_similarity" ? "штрих, без стрелки" : "сплошная, без стрелки"}</small></div>
          {/each}
          {#if query}
            <div class="row"><svg width="34" height="12" viewBox="0 0 34 12" aria-hidden="true"><line x1="1" y1="6" x2="29" y2="6" stroke={palette?.kindCss.query_result ?? "currentColor"} stroke-width="2.2" /></svg>{KIND_TITLE.query_result}<small>оценка поиска; без оценки — пунктир</small></div>
          {/if}
          <p class="hint">Взаимные ссылки — одна линия со стрелками на обоих концах. В покое линии серые и тонкие; цвет вида — у связей выбранного или наведённого узла. Размер узла — число связей, цвет — модуль.</p>
          <h4>Сила связи (s)</h4>
          <p class="hint">Толщина и яркость растут с силой; шкала абсолютная, от других рёбер экрана не зависит.</p>
          <div class="scale">
            {#each sampleS as s (s)}
              {@const e = { kind: "document_similarity" as EdgeKind, s, raw: s }}
              <div><svg width="40" height="8" viewBox="0 0 40 8" aria-hidden="true"><line x1="2" y1="4" x2="38" y2="4" stroke={palette?.kindCss.document_similarity} stroke-opacity={brightnessOf(drawStrength(e))} stroke-width={widthOf(drawStrength(e))} /></svg><span>cos {s.toFixed(2)}</span></div>
            {/each}
          </div>
          <p class="hint">Значения разных видов не сопоставимы как мера зависимости. Близость — не доказанная зависимость.</p>
          <h4>Узлы</h4>
          <div class="row"><svg width="34" height="12" viewBox="0 0 34 12" aria-hidden="true"><circle cx="8" cy="6" r="4.5" fill="var(--muted)" /><path d="M24 0.5l5.5 5.5-5.5 5.5-5.5-5.5z" fill="var(--faint)" /></svg>документ · чанк</div>
          <div class="row"><svg width="34" height="12" viewBox="0 0 34 12" aria-hidden="true"><circle cx="8" cy="6" r="4.5" fill="none" stroke="var(--accent)" stroke-width="2" /><rect x="22" y="1" width="4" height="4" fill="var(--accent)" /></svg>выбран · закреплён</div>
        </section>
      {/if}
    {:else}
      <div class="list">
        <div class="list-tools">
          <input type="search" placeholder="Фильтр списка" aria-label="Фильтр списка узлов" bind:value={listFilter} />
          <label class="chk"><input type="checkbox" bind:checked={isolatesOnly} /> без связей</label>
          <span class="count">{rows.length}</span>
        </div>
        {#if rendererError}<p class="note" role="alert">{rendererError}</p>{/if}
        <ul aria-label="Узлы графа">
          {#each rows.slice(0, 400) as n (n.index)}
            <li>
              <button class:on={selected?.type === "node" && selected.index === n.index} onclick={() => select({ type: "node", index: n.index })}>
                <span class="glyph {n.node.kind}"></span><span class="name" title={n.node.label}>{n.node.label}</span><small>{n.node.module}</small><b>{n.degree}</b>
              </button>
            </li>
          {/each}
        </ul>
      </div>
    {/if}
  </div>

  {#if !mini}
    <aside class="inspector" aria-label="Инспектор графа">
      {#if selectedNode}
        <header>
          <span class="kind">{selectedNode.kind === "document" ? "Документ" : selectedNode.kind === "query" ? "Запрос" : "Чанк"}</span>
          <h3>{selectedNode.label}</h3>
          {#if selectedNode.kind !== "query"}<p class="path">{selectedNode.path} · модуль {selectedNode.module}{#if selectedNode.kind === "chunk"} · строки {selectedNode.firstLine}–{selectedNode.lastLine}{/if}</p>{/if}
          {#if selectedNode.kind === "chunk"}<p class="path">Владелец: <button class="inline" onclick={() => select({ type: "node", index: selectedNode.owner })}>{label(selectedNode.owner)}</button></p>{/if}
        </header>
        <div class="actions">
          {#if selectedNode.kind !== "query"}<button class="act" disabled={query?.results.some(r => r.node === selectedNode.index && (r.status === "deleted_since_search" || r.status === "unavailable"))} onclick={() => openSource(selectedNode.index)}><Icon name="doc" size={13} width={1.8} /> {selectedNode.kind === "document" ? "Открыть" : "Открыть диапазон"}</button>{/if}
          {#if selectedNode.kind === "document" && selectedNode.chunks && !query}
            <button class="act" onclick={() => void toggleExpand(selectedNode.index)} disabled={loading}>{expanded[selectedNode.index] ? "Свернуть чанки" : `Раскрыть чанки (${selectedNode.chunks})`}</button>
          {/if}
          <button class="act" onclick={() => pin(selectedNode.index)} aria-pressed={pinnedSel}>{pinnedSel ? "Открепить" : "Закрепить"}</button>
        </div>
        {#if selectedNode.kind === "document" && expanded[selectedNode.index] && expanded[selectedNode.index] < selectedNode.chunks}
          <p class="trunc" role="status">Показано {expanded[selectedNode.index]} из {selectedNode.chunks} чанков. <button class="inline" onclick={() => void moreChunks(selectedNode.index)} disabled={loading}>Ещё {Math.min(PORTION, selectedNode.chunks - expanded[selectedNode.index])}</button></p>
        {/if}
        {#if selectedNode.kind === "query" && query}
          <p class="origin">Запрос поиска. Режим «{SEARCH_MODE[query.applied]}», {SCORE_TEXT[query.scoreKind]}. Граф по тексту запроса ничего не ищет: на карте документы выдачи и их окружение по ссылкам и по смыслу.</p>
          <h4>Результаты · {query.results.length}</h4>
          <ul class="conns">
            {#each query.results as r (r.rank)}
              <li>
                <button disabled={r.node === null} title={r.reason ?? r.path} onclick={() => r.node !== null && select({ type: "node", index: r.node })}>
                  <span class="name">{r.rank}. {r.title}</span><small>{r.score === null ? "—" : r.score.toFixed(2)}{r.channel === "related" ? " · связанный источник" : ""} · {SOURCE_TEXT[r.status]}{r.node === null ? ` · ${r.reason}` : ""}</small>
                </button>
              </li>
            {/each}
          </ul>
        {:else if selectedNode.qrole && query}
          {@const found = query.results.find((x) => x.node === selectedNode.index)}
          {#if found}
            <p class="origin">{SOURCE_TEXT[found.status]} · {found.channel === "related" ? "Связанный источник без оценки запроса" : `Результат поиска №${found.rank}`}{found.score !== null ? ` · ${SCORE_TEXT[found.kind]} ${found.score.toFixed(2)}` : ""}. Совпавших фрагментов: {found.fragments.length}.</p>
            <ul class="conns">
              {#each found.fragments as f, i (i)}<li><span class="name" title={f.heading}>{f.heading || "без заголовка"}</span><small>строки {f.first}–{f.last} · {f.score === null ? "оценки нет" : f.score.toFixed(2)}</small></li>{/each}
            </ul>
          {/if}
        {/if}
        <h4>Связи · {connections.length}</h4>
        {#each CONN_KINDS as kind (kind)}
          {@const items = connections.filter((c) => c.edge.kind === kind)}
          {#if items.length}
            <h5>{KIND_TITLE[kind]} · {items.length}</h5>
            <ul class="conns">
              {#each items.slice(0, 40) as c (c.edge.id)}
                {@const line = edgeLine(c.edge)}
                <li>
                  <button onclick={() => select({ type: "edge", id: c.edge.id })} title="Показать основание связи">
                    <svg width="30" height="10" viewBox="0 0 30 10" aria-hidden="true"><line x1="1" y1="5" x2="27" y2="5" stroke={palette?.kindCss[kind]} stroke-opacity={line.b} stroke-width={line.w} stroke-dasharray={dash(kind)} />{#if kind === "markdown_link"}<path d="M29 5l-4-2.4v4.8z" fill={palette?.kindCss[kind]} />{/if}</svg>
                    <span class="name">{kind === "markdown_link" ? (c.edge.a === selectedNode.index ? "→ " : "← ") : ""}{label(c.other)}</span>
                    <small>{c.edge.raw === null ? (c.edge.kind === "query_result" ? "без оценки" : "ссылка") : c.edge.raw.toFixed(2)}</small>
                  </button>
                </li>
              {/each}
            </ul>
            {#if items.length > 40}<p class="trunc">Показаны 40 из {items.length}; остальные — в списке карты.</p>{/if}
          {/if}
        {/each}
        {#if !connections.length}<p class="hint">У узла нет связей в выбранных видах и области. Он остаётся в списке.</p>{/if}
      {:else if selectedEdge}
        <header>
          <span class="kind">{KIND_TITLE[selectedEdge.kind]}</span>
          <h3>{edgeTitle(selectedEdge)}</h3>
        </header>
        <p class="origin">{originText(selectedEdge)}</p>
        <table aria-label="Числа связи">
          <tbody>
            <tr><th>{selectedEdge.kind === "query_result" ? "Оценка поиска" : "Исходный cosine"}</th><td>{selectedEdge.raw === null ? (selectedEdge.kind === "query_result" ? "нет" : "нет (ссылка)") : selectedEdge.raw.toFixed(3)}</td></tr>
            {#if selectedEdge.kind === "markdown_link"}<tr><th>Строки в тексте</th><td>{linesText(selectedEdge)}</td></tr>{/if}
            <tr><th>Сила s</th><td>{selectedEdge.s.toFixed(3)}{selectedEdge.kind === "markdown_link" ? " (факт наличия)" : ""}</td></tr>
            <tr><th>Толщина</th><td>{widthOf(drawStrength(selectedEdge)).toFixed(2)} px</td></tr>
            <tr><th>Яркость</th><td>{pct(brightnessOf(drawStrength(selectedEdge)))} цвета вида</td></tr>
            <tr><th>Притяжение q(s)</th><td>{springOf(selectedEdge.s).toFixed(3)}</td></tr>
            <tr><th>Пружина после ограничения</th><td>{springOfSelected === null ? "—" : springOfSelected.toFixed(3)}</td></tr>
          </tbody>
        </table>
        <p class="hint">{selectedEdge.kind === "markdown_link" ? "Все ссылки имеют одну силу: частота не показывает важность." : selectedEdge.kind === "query_result" ? "Оценка поиска — не вероятность правильного ответа; rerank и косинус не сопоставимы." : "Близость не доказывает зависимость и не заменяет ссылку."} Значения разных видов не сопоставимы.</p>
        <div class="actions">
          <button class="act" onclick={() => select({ type: "node", index: selectedEdge.a })}>Узел: {label(selectedEdge.a).slice(0, 22)}</button>
          <button class="act" onclick={() => select({ type: "node", index: selectedEdge.b })}>Узел: {label(selectedEdge.b).slice(0, 22)}</button>
          {#if selectedEdge.kind === "markdown_link" && selectedEdge.lines.length}<button class="act" onclick={() => openSource(selectedEdge.a, selectedEdge.lines[0])}><Icon name="doc" size={13} width={1.8} /> Источник ссылки</button>{/if}
        </div>
      {:else}
        <header><span class="kind">Граф проекта</span><h3>Выберите документ</h3></header>
        <p class="hint">Щёлкните узел или найдите документ по названию — граф покажет, что с ним связано, и откроет источник. Движение камеры область поиска не меняет.</p>
        <details class="key-help">
          <summary>Как управлять картой с клавиатуры</summary>
          <dl>
            <dt>Стрелки</dt><dd>Выбрать соседний узел</dd>
            <dt>M</dt><dd>Следующий режим связей</dd>
            <dt>Enter</dt><dd>Раскрыть чанки документа</dd>
            <dt>Пробел</dt><dd>Закрепить или открепить</dd>
            <dt>F</dt><dd>Вписать карту</dd>
            <dt>Esc</dt><dd>Снять выбор</dd>
          </dl>
        </details>
      {/if}
      {#if query?.bundle}
        <GraphSearchBundle graph={query.bundle} states={Object.fromEntries(query.results.map(r => [r.path,r.status]))} related={query.applied === "mix"} embedded selected={packageIndex} onselect={(index) => { packageIndex = index; schedule(); }} />
      {/if}
      {#if notice}<p class="notice" role="status">{notice}</p>{/if}
    </aside>
  {/if}
</div>
</div>

<style>
  .pgw { display: flex; flex: 1; min-width: 0; min-height: 0; height: 100%; container-type: inline-size; }
  .pg { position: relative; flex: 1; display: grid; grid-template-columns: minmax(0, 1fr) 300px; grid-template-rows: auto minmax(0, 1fr); flex: 1; min-height: 0; min-width: 0; height: 100%; background: var(--panel); font-size: 12px; color: var(--text); overflow: hidden; }
  .pg.mini { display: flex; flex-direction: column; }
  .top { grid-column: 1 / -1; grid-row: 1; display: flex; flex-direction: column; min-width: 0; }
  .entry-actions { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; padding: 4px 10px; border-bottom: 1px solid var(--line); font-size: 11.5px; }
  .entry-title { max-width: 32ch; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .entry-hint { flex: 1 1 260px; color: var(--muted); }
  .banner.state .ghost { background: transparent; }
  .banner .more:disabled { opacity: 0.5; cursor: default; }
  .facts { margin-top: 6px; font-size: 11px; }
  .facts summary { cursor: pointer; color: var(--muted); }
  .key-help { margin-top: 8px; font-size: 11px; color: var(--muted); }
  .key-help summary { cursor: pointer; }
  .key-help dl { display: grid; grid-template-columns: auto 1fr; gap: 2px 10px; margin: 6px 0 0; }
  .key-help dt { font-weight: 600; color: var(--text); }
  .key-help dd { margin: 0; }
  .bar { grid-column: 1 / -1; display: flex; align-items: center; gap: 8px; padding: 4px 8px; border-bottom: 1px solid var(--line); flex-wrap: wrap; }
  .banner { grid-column: 1 / -1; display: flex; gap: 8px 14px; align-items: center; flex-wrap: wrap; padding: 4px 10px; background: var(--mid-soft); color: var(--text); border-bottom: 1px solid var(--line); font-size: 11.5px; }
  .qstack { grid-column: 1 / -1; display: flex; flex-direction: column; }
  .qstack .banner { grid-column: auto; }
  .qbar { grid-column: 1 / -1; display: flex; align-items: center; gap: 6px 14px; flex-wrap: wrap; padding: 4px 10px; background: var(--accent-soft); border-bottom: 1px solid var(--line); font-size: 11.5px; }
  .qbar .qtext { max-width: 38ch; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .qbar .more { border: 1px solid var(--line-2); background: var(--panel); border-radius: 5px; padding: 1px 8px; font-size: 11.5px; margin-left: auto; }
  .qbar .depth { display: inline-flex; align-items: center; gap: 4px; }
  .qbar .depth button { width: 20px; height: 20px; border: 1px solid var(--line-2); background: var(--panel); border-radius: 4px; font-size: 11px; }
  .qbar .depth button.on { background: var(--accent); border-color: var(--accent); color: #fff; }
  .qsq { display: inline-block; width: 9px; height: 9px; background: #d9730d; flex: none; }
  .qplate { position: absolute; left: 6px; top: 4px; display: flex; align-items: center; gap: 6px; max-width: calc(100% - 90px); padding: 2px 8px; border: 1px solid var(--line); border-radius: 6px; background: var(--panel); font-size: 11px; }
  .qplate.dashed { border-style: dashed; }
  .qplate button { white-space: nowrap; }
  .qplate .qt { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .banner .more { border: 1px solid var(--line-2); background: var(--panel); border-radius: 5px; padding: 1px 8px; font-size: 11.5px; }
  .stage { grid-column: 1; grid-row: 2; position: relative; min-width: 0; min-height: 0; overflow: hidden; background: var(--panel); }
  .pg.mini .stage { flex: 1; }
  canvas { position: absolute; inset: 0; width: 100%; height: 100%; display: block; cursor: grab; touch-action: none; outline: none; }
  canvas.hide { visibility: hidden; }
  canvas:focus-visible { box-shadow: inset 0 0 0 2px var(--accent); }
  .find { display: flex; align-items: center; gap: 4px; height: 24px; padding: 0 6px; border: 1px solid var(--line-2); border-radius: 6px; color: var(--muted); }
  .find input { border: 0; background: transparent; outline: 0; width: 120px; font-size: 11px; }
  .found { font-size: 10.5px; color: var(--accent-text); }
  .modes { display: flex; gap: 0; flex-wrap: wrap; flex-basis: 100%; }
  .mode { display: inline-flex; align-items: center; gap: 5px; height: 24px; padding: 0 9px; border: 1px solid var(--line-2); background: var(--panel); color: var(--muted); font-size: 11px; margin-left: -1px; }
  .mode:first-child { border-radius: 6px 0 0 6px; margin-left: 0; }
  .mode:last-child { border-radius: 0 6px 6px 0; }
  .mode[aria-checked="true"] { background: var(--accent-soft); color: var(--accent-text); border-color: var(--accent); position: relative; z-index: 1; }
  .mode:disabled { opacity: 0.5; cursor: default; }
  .mode b { font-weight: 600; }
  .mode .state { font-size: 10px; padding: 0 5px; border-radius: 8px; background: var(--mid-soft); color: var(--mid); }
  .mode .state.failed { background: var(--bad-soft); color: var(--bad); }
  .toast { position: absolute; left: 6px; top: 6px; max-width: calc(100% - 150px); padding: 4px 10px; border-radius: 8px; background: var(--text); color: var(--panel); font-size: 11.5px; line-height: 1.35; box-shadow: var(--shadow); pointer-events: none; z-index: 6; animation: toast-in 0.18s ease-out; }
  @keyframes toast-in { from { opacity: 0; transform: translateY(-4px); } }
  @media (prefers-reduced-motion: reduce) { .toast { animation: none; } }
  .mini-tools .mode-btn { color: var(--accent-text); }
  .sep { flex: 1; }
  .tools { display: flex; gap: 1px; padding: 2px; border: 1px solid var(--line); border-radius: 5px; background: var(--panel); }
  .tools button, .mini-tools button { display: grid; place-items: center; width: 20px; height: 20px; padding: 0; border: 0; border-radius: 4px; background: transparent; color: var(--muted); }
  .tools button:hover, .mini-tools button:hover { background: var(--accent-soft); color: var(--accent-text); }
  .tools button[aria-pressed="true"], .tools button[aria-expanded="true"] { background: var(--accent-soft); color: var(--accent-text); }
  .tools button:disabled, .mini-tools button:disabled { opacity: 0.4; cursor: default; }
  button:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .status { position: absolute; left: 8px; bottom: 6px; display: flex; align-items: center; gap: 6px; font-size: 10.5px; color: var(--muted); background: color-mix(in srgb, var(--panel) 85%, transparent); padding: 1px 6px; border-radius: 6px; max-width: calc(100% - 16px); }
  .status b { white-space: nowrap; }
  .status .lt { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--faint); flex: none; }
  .dot.running { background: var(--accent); animation: pulse 1.2s infinite; }
  .dot.stopped { background: var(--mid); }
  @keyframes pulse { 50% { opacity: 0.35; } }
  @media (prefers-reduced-motion: reduce) { .dot.running { animation: none; } }
  .inline { border: 0; background: none; padding: 0; color: var(--accent-text); font-size: inherit; text-decoration: underline; text-underline-offset: 2px; }
  .tip { position: absolute; max-width: 240px; padding: 3px 7px; border-radius: 6px; background: var(--text); color: var(--panel); font-size: 11px; pointer-events: none; z-index: 3; box-shadow: var(--shadow); }
  .mini-tools { position: absolute; top: 4px; right: 4px; display: flex; gap: 1px; padding: 2px; border: 1px solid var(--line); border-radius: 5px; background: var(--panel); opacity: 0; transition: opacity 0.15s; }
  .pg:hover .mini-tools, .mini-tools:focus-within, .pg:has(.pop) .mini-tools { opacity: 1; }
  .mini-tools button[aria-expanded="true"] { background: var(--accent-soft); color: var(--accent-text); }
  .caption { position: absolute; left: 6px; right: 6px; bottom: 24px; display: flex; align-items: center; gap: 8px; padding: 3px 8px; border: 1px solid var(--line); border-radius: 6px; background: var(--panel); box-shadow: var(--shadow); font-size: 11px; }
  .caption b { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .caption span { color: var(--muted); white-space: nowrap; }
  .empty { position: absolute; inset: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 4px; padding: 20px; text-align: center; color: var(--muted); margin: 0; }
  .note { position: absolute; top: 8px; left: 8px; right: 8px; margin: 0; padding: 4px 8px; border-radius: 6px; background: var(--mid-soft); color: var(--text); font-size: 11.5px; }
  .legend { position: absolute; top: 8px; right: 8px; width: 250px; max-height: calc(100% - 16px); overflow: auto; padding: 8px 10px; border: 1px solid var(--line-2); border-radius: 8px; background: var(--panel); box-shadow: var(--shadow); z-index: 4; }
  .legend h4, .inspector h4 { margin: 8px 0 3px; font-size: 10.5px; letter-spacing: 0.02em; color: var(--muted); font-weight: 600; }
  .pop { position: absolute; top: 30px; right: 4px; width: min(230px, calc(100% - 8px)); padding: 6px 8px; border: 1px solid var(--line-2); border-radius: 8px; background: var(--panel); box-shadow: var(--shadow); z-index: 5; }
  .pop.full { top: 8px; right: 8px; width: 260px; }
  .pop.menu { top: 34px; width: 230px; padding: 4px; display: flex; flex-direction: column; }
  .pop.menu button { display: flex; flex-direction: column; align-items: flex-start; gap: 1px; padding: 5px 8px; border: 0; border-radius: 5px; background: transparent; color: var(--text); font-size: 12px; text-align: left; }
  .pop.menu button:hover { background: var(--accent-soft); color: var(--accent-text); }
  .pop.menu small { font-size: 10.5px; color: var(--muted); }
  .tools .more-menu { font-size: 14px; line-height: 1; }
  .pop h4 { margin: 0 0 4px; font-size: 10.5px; color: var(--muted); font-weight: 600; }
  .opt { display: flex; align-items: center; gap: 6px; margin: 3px 0; font-size: 11px; }
  .opt span { flex: 1; min-width: 0; }
  .opt b { font-variant-numeric: tabular-nums; color: var(--muted); font-weight: 600; min-width: 30px; text-align: right; }
  .opt input[type="range"] { width: 62px; accent-color: var(--accent); flex: none; }
  .slider span { flex: none; width: 104px; }
  .thr-row span { flex: none; }
  .reset { margin-top: 4px; border: 1px solid var(--line-2); background: transparent; color: var(--muted); border-radius: 5px; padding: 1px 8px; font-size: 11px; }
  .legend h4:first-child { margin-top: 0; }
  .legend .row { display: flex; align-items: center; gap: 8px; margin: 2px 0; }
  .legend small { margin-left: auto; color: var(--faint); font-size: 10.5px; }
  .legend .scale { display: flex; gap: 8px; }
  .legend .scale div { display: grid; justify-items: center; font-size: 10.5px; color: var(--muted); }
  .hint { margin: 4px 0; color: var(--muted); font-size: 11px; line-height: 1.4; }
  .inspector { grid-column: 2; grid-row: 2; min-height: 0; overflow: auto; padding: 8px 10px; border-left: 1px solid var(--line); background: var(--panel); }
  .inspector header { margin-bottom: 6px; }
  .inspector .kind { font-size: 10.5px; text-transform: uppercase; letter-spacing: 0.04em; color: var(--accent-text); font-weight: 600; }
  .inspector h3 { margin: 1px 0 2px; font-size: 13px; line-height: 1.3; overflow-wrap: anywhere; }
  .inspector h5 { margin: 6px 0 2px; font-size: 11px; color: var(--muted); font-weight: 600; }
  .path { margin: 0; color: var(--muted); font-size: 11px; overflow-wrap: anywhere; }
  .actions { display: flex; gap: 4px; flex-wrap: wrap; margin: 6px 0; }
  .act { display: inline-flex; align-items: center; gap: 5px; padding: 2px 8px; height: 24px; border: 1px solid var(--line-2); border-radius: 6px; background: transparent; color: var(--text); font-size: 11px; }
  .act:hover, .act[aria-pressed="true"] { background: var(--accent-soft); color: var(--accent-text); border-color: transparent; }
  .conns { list-style: none; margin: 0; padding: 0; }
  .conns button, .list li button { display: flex; align-items: center; gap: 6px; width: 100%; padding: 2px 4px; border: 0; border-radius: 5px; background: transparent; text-align: left; font-size: 11.5px; }
  .conns button:hover, .list li button:hover { background: var(--panel-2); }
  .conns .name, .list .name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .conns small, .list small { color: var(--muted); font-size: 10.5px; }
  .origin { margin: 4px 0; line-height: 1.45; }
  table { width: 100%; border-collapse: collapse; margin: 6px 0; font-size: 11.5px; }
  th { text-align: left; font-weight: 500; color: var(--muted); padding: 2px 6px 2px 0; }
  td { text-align: right; font-variant-numeric: tabular-nums; }
  tr + tr { border-top: 1px solid var(--line); }
  .trunc { margin: 4px 0; padding: 3px 6px; border-radius: 5px; background: var(--mid-soft); font-size: 11px; }
  .notice { margin: 8px 0 0; padding: 4px 6px; border-radius: 5px; background: var(--accent-soft); color: var(--accent-text); font-size: 11px; }
  .list { position: absolute; inset: 0; display: flex; flex-direction: column; min-height: 0; }
  .list-tools { display: flex; gap: 8px; align-items: center; padding: 6px 8px; border-bottom: 1px solid var(--line); }
  .list-tools input[type="search"] { flex: 1; height: 24px; border: 1px solid var(--line-2); border-radius: 6px; padding: 0 6px; background: transparent; font-size: 11px; }
  .chk { display: flex; align-items: center; gap: 5px; font-size: 11px; color: var(--muted); }
  .count { color: var(--muted); font-size: 11px; }
  .list .note { position: static; margin: 6px 8px; }
  .list ul { list-style: none; margin: 0; padding: 4px 6px; overflow: auto; flex: 1; }
  .list li button.on { background: var(--accent-soft); color: var(--accent-text); }
  .glyph { width: 8px; height: 8px; flex: none; background: var(--muted); border-radius: 50%; }
  .glyph.chunk { border-radius: 1px; transform: rotate(45deg) scale(0.8); background: var(--faint); }
  @container (max-width: 640px) {
    .pg:not(.mini) { grid-template-columns: minmax(0, 1fr); grid-template-rows: auto minmax(0, 1fr) minmax(0, 38%); }
    .stage { grid-column: 1; }
    .inspector { grid-column: 1; grid-row: 3; border-left: 0; border-top: 1px solid var(--line); }
    .find input { width: 90px; }
  }
</style>
