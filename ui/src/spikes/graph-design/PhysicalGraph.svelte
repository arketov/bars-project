<script lang="ts">
  // Панель физического графа — стенд дизайна 2.1.4. Заменяет GraphPage внутри настоящего окна (см. vite.graph-design.config.ts):
  // мини-граф под навигацией и полный граф вкладкой используют один и тот же компонент.
  import { onDestroy, onMount, untrack } from "svelte";
  import Icon from "../../lib/Icon.svelte";
  import { app } from "../../lib/app.svelte";
  import { PORTION, loadSlice, parseFlags, queryView, shapeQuery, type QueryResult, type Slice } from "./data";
  import { draw, layoutLines, lineDistance, readPalette, toScreen, toWorld, type Camera, type Palette, type Scene, type Target } from "./draw";
  import { DEFAULT_FORCES, Layout, type Forces, type PNode } from "./layout";
  import { ALL_KINDS, EDGE_KINDS, KIND_TITLE, brightnessOf, drawStrength, hash, springOf, widthOf, type EdgeKind, type GEdge, type Mode, type ScaleName } from "./model";

  const params = new URLSearchParams(location.search);
  const scenario = params.get("scenario") ?? "project";
  // Шкала рисунка по умолчанию — окно модели: у близости 0,6–0,95 по проектной шкале толщина почти одинакова.
  let scale = $state<ScaleName>(params.get("scale") === "project" ? "project" : "window");
  const reduced = params.get("motion") === "reduced" || (typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches);

  let baseSlice = $state.raw<Slice>(loadSlice(scenario));
  let host: HTMLDivElement;
  let stage: HTMLDivElement;
  let canvas = $state<HTMLCanvasElement | null>(null);
  let mini = $state(false);
  let size = $state({ w: 0, h: 0 });
  /** Порог показа близости: фильтр отбора, сила остальных рёбер от него не меняется. */
  let minS = $state(params.has("smin") ? Number(params.get("smin")) : 0.82);
  /** Режим связей: какие виды видны вместе. Один вид за раз — «Ссылки», «По смыслу», «По смыслу · чанки»; «Все связи» — вместе. */
  const MODE_PARAM: Record<string, Mode> = { links: "links", meaning: "meaning", chunks: "chunks", all: "all", markdown_link: "links", document_similarity: "meaning", chunk_similarity: "chunks" };
  let mode = $state<Mode>(MODE_PARAM[params.get("mode") ?? ""] ?? "links");
  const modeKinds = (m: Mode): EdgeKind[] => (m === "links" ? ["markdown_link"] : m === "meaning" ? ["document_similarity"] : m === "chunks" ? ["chunk_similarity"] : ["markdown_link", "document_similarity", "chunk_similarity"]);
  const kinds = $derived.by(() => {
    const on = modeKinds(mode);
    return { markdown_link: on.includes("markdown_link"), document_similarity: on.includes("document_similarity"), chunk_similarity: on.includes("chunk_similarity"), query_result: true } as Record<EdgeKind, boolean>;
  });
  /**
   * Вид запроса (Т-47, query-graph.md): запрос — узел, от него связи с результатами; от результатов — связи выбранного режима.
   * `latest` — последний завершённый поиск; `queryOn` — показан ли его вид. Идущий поиск и ошибка вид не меняют.
   */
  const flags = parseFlags(params.get("qs"));
  let latest = $state<{ text: string; id: number } | null>(params.get("query")?.trim() ? { text: params.get("query")!.trim(), id: 1 } : null);
  let queryOn = $state(!!params.get("query")?.trim());
  const queryActive = $derived(queryOn && latest !== null);
  const queryText = $derived(latest?.text ?? "");
  /** Глубина окружения результатов: 0 — запрос и результаты, 1 по умолчанию, 2 — максимум (OQ-119). */
  let depth = $state(params.has("depth") ? Math.min(2, Math.max(0, Number(params.get("depth")))) : 1);
  /** Сколько раз нажато «Показать ещё»: пределы растут в том же снимке. */
  let more = $state(0);
  const qslice = $derived(latest ? queryView(baseSlice, latest.text, latest.id, flags) : null);
  const slice = $derived(queryActive && qslice ? qslice : baseSlice);
  const qinfo = $derived(queryActive ? (slice.queryInfo ?? null) : null);
  const budget = $derived.by(() => {
    if (!qinfo) return { nodes: 0, edges: 0 };
    const b = qinfo.budget;
    return { nodes: Math.min(b.maxNodes, b.nodes + more * Math.ceil((b.maxNodes - b.nodes) / 2)), edges: Math.min(b.maxEdges, b.edges + more * Math.ceil((b.maxEdges - b.edges) / 2)) };
  });
  const shape = $derived(qinfo ? shapeQuery(slice, modeKinds(mode), minS, depth, budget) : null);
  const canMore = $derived(!!qinfo && !!shape?.truncated && (budget.nodes < qinfo.budget.maxNodes || budget.edges < qinfo.budget.maxEdges));
  /** Имитация состояний поиска для согласования: qstate=searching|error. В окне их даёт настоящий поиск (`app.searching`, `app.searchError`). */
  const pendingSearch = $derived(app.searching || params.get("qstate") === "searching");
  const failedSearch = $derived(!!app.searchError || params.get("qstate") === "error");
  const failedText = $derived(app.searchError?.message ?? "стенд: имитация ошибки поиска");
  const resultNodes = $derived(qinfo ? qinfo.results.filter((r) => r.onMap && r.channel !== "related") : []);
  const offMap = $derived(qinfo ? qinfo.results.filter((r) => !r.onMap) : []);
  const ghosts = $derived(qinfo ? qinfo.results.filter((r) => r.status !== "ready") : []);
  const SCORE_LABEL = { rerank: "оценка rerank", cosine: "оценка — косинус (rerank не выполнялся)", none: "оценки нет" } as const;
  const MODE_SEARCH = { hybrid: "Обычный", mix: "Со связями", graph: "Отношения и пути" } as const;
  const qWarnings = $derived.by(() => {
    const list: string[] = [];
    if (!qinfo) return list;
    if (qinfo.fallbackNote) list.push(qinfo.fallbackNote);
    if (qinfo.changedSinceSearch) list.push("Вид изменён после поиска: индекс обновился. Показан снимок поиска, он не смешан со свежими данными.");
    if (ghosts.length) list.push(`Граф неполон: результатов без объекта графа — ${ghosts.length}. Они остаются в выдаче и на карте пустыми узлами с причиной.`);
    if (offMap.length) list.push(`Связанных источников вне карты: ${offMap.length} — физической связи с результатами нет; они в списке инспектора запроса.`);
    return list;
  });
  let toast = $state("");
  let toastTimer = 0;
  let expanded = $state<Record<number, number>>({});
  const shownChunks = $derived(Object.values(expanded).reduce((a, b) => a + b, 0));
  let selected = $state<Target>(null);
  let tip = $state<{ text: string; x: number; y: number } | null>(null);
  let motion = $state<"running" | "idle" | "stopped" | "reduced">(reduced ? "reduced" : "idle");
  let stopped = $state(false);
  let view = $state<"map" | "list">(scenario === "norender" ? "list" : "map");
  let listFilter = $state("");
  let isolatesOnly = $state(false);
  let findText = $state("");
  let legendOpen = $state(false);
  /** Открытое меню: виды связей (мини-граф) или настройки раскладки. */
  let popover = $state<"settings" | null>(null);
  let forces = $state<Forces>({ ...DEFAULT_FORCES });
  let notice = $state("");
  let rendererError = $state(scenario === "norender" ? "Canvas недоступен: renderer не создан. Карта заменена списком, связи и источники доступны." : "");
  let revision = $state(0);

  let layout: Layout | null = null;
  let palette = $state.raw<Palette | null>(null);
  let cam: Camera = { x: 0, y: 0, k: 1 };
  let fitted = false;
  let pendingFocus: { index: number; k: number } | null = null;
  let lastQueryKey = "";
  let fitTimer = 0;
  /** Общий вид при входе в вид запроса: места узлов, камера и выбор возвращаются кнопкой «Общий вид». */
  let generalSnap: { pos: Map<number, [number, number]>; cam: Camera; selected: Target; w: number; h: number } | null = null;
  let raf = 0;
  let hidden = false;
  let hovered: Target = null;
  let hostObserver: ResizeObserver | null = null;
  let themeObserver: MutationObserver | null = null;
  let io: IntersectionObserver | null = null;

  const owner = (index: number) => slice.nodes[index].owner;
  const label = (index: number) => slice.nodes[index].label;

  function visibleNodes() {
    if (queryActive && shape) return shape.nodes.map((i) => slice.nodes[i]);
    const nodes = slice.nodes.filter((n) => n.kind === "query" || (n.kind === "document" && (!slice.visibleDocs || slice.visibleDocs.has(n.index))));
    if (!mini && !queryActive) for (const [o, shown] of Object.entries(expanded)) for (const c of slice.chunksOf[Number(o)].slice(0, shown)) nodes.push(slice.nodes[c]);
    return nodes;
  }

  const passes = (e: GEdge) => e.kind === "markdown_link" || e.kind === "query_result" || e.s >= minS;

  function applyForces() {
    if (!layout) return;
    layout.setForces($state.snapshot(forces) as Forces);
    if (reduced) layout.settle(600);
    else if (!stopped) layout.reheat(0.5);
    revision++;
    schedule();
  }

  function resetForces() {
    forces = { ...DEFAULT_FORCES };
    applyForces();
  }

  function onThreshold() {
    rebuild(0.35);
  }

  function visibleEdges(nodes = visibleNodes()): GEdge[] {
    const set = new Set(nodes.map((n) => n.index));
    return slice.edges.filter((e) => kinds[e.kind] && passes(e) && !(mini && e.kind === "chunk_similarity") && set.has(e.a) && set.has(e.b) && (!queryActive || !shape?.edgeIds || e.kind === "query_result" || shape.edgeIds.has(e.id)));
  }

  /** Начальные места вида запроса: запрос в центре, результаты по кругу в порядке выдачи, окружение около своих результатов. */
  function seedPlan(): Map<number, [number, number]> | undefined {
    if (!queryActive || !shape || !qinfo) return undefined;
    const plan = new Map<number, [number, number]>([[qinfo.queryNode, [0, 0]]]);
    const radius = 70 + 5 * resultNodes.length;
    resultNodes.forEach((r, i) => {
      const a = -Math.PI / 2 + (i / Math.max(1, resultNodes.length)) * Math.PI * 2;
      plan.set(r.node, [Math.cos(a) * radius, Math.sin(a) * radius]);
    });
    for (const n of shape.nodes) {
      if (plan.has(n)) continue;
      const anchor = plan.get(shape.anchorOf.get(n) ?? -1);
      if (!anchor) continue;
      const away = Math.atan2(anchor[1], anchor[0]) + (hash(n, 3) - 0.5) * Math.PI * 1.3;
      const r = 36 * (shape.depthOf.get(n) ?? 1) + hash(n, 4) * 28;
      plan.set(n, [anchor[0] + Math.cos(away) * r, anchor[1] + Math.sin(away) * r]);
    }
    return plan;
  }

  /** `fresh` — переложить с нуля (смена режима или вида): расположение определяют только текущие связи. */
  function rebuild(reheat = 0.55, fresh = false) {
    if (!layout) return;
    const nodes = visibleNodes();
    layout.setGraph(nodes, visibleEdges(nodes), owner);
    if (fresh) {
      layout.reseed();
      const plan = seedPlan();
      if (plan) layout.place(plan);
      // Остановка и reduced motion: раскладка считается сразу, без анимации; иначе — короткая подготовка и дальше движение.
      if (reduced || stopped) layout.settle(600);
      else {
        layout.settle(40);
        layout.reheat(0.8);
      }
      clearTimeout(fitTimer);
      fitTimer = window.setTimeout(fit, 120);
    } else if (reduced) layout.settle(600);
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
    const moving = !stopped && !reduced && !hidden && layout.active;
    if (moving) layout.step(2);
    paint();
    const still = !stopped && !reduced && !hidden && layout.active;
    const next = reduced ? "reduced" : stopped ? "stopped" : still ? "running" : "idle";
    if (next !== motion) motion = next;
    if (still) schedule();
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
      if (picked.kind === "document") for (const c of slice.chunksOf[picked.index].slice(0, expanded[picked.index] ?? 0)) set.add(c);
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

  function paint() {
    if (!canvas || !layout || !palette || size.w < 2) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      rendererError = "Canvas недоступен: карта заменена списком.";
      view = "list";
      return;
    }
    const groups = Object.entries(expanded).map(([o, shown]) => ({ owner: Number(o), shown, total: slice.chunksOf[Number(o)].length }));
    const scene: Scene = {
      w: size.w, h: size.h, dpr: window.devicePixelRatio || 1, cam, nodes: layout.nodes, links: layout.links, palette, scale, kinds,
      selected, hovered, related: related(), marked: markedNodes(), groups, mini,
    };
    draw(ctx, scene);
  }

  function fit() {
    if (!layout || size.w < 2) return;
    const b = layout.bounds();
    if (!b) return;
    const pad = mini ? 14 : 36;
    const k = Math.min(1.6, Math.max(0.12, Math.min((size.w - pad * 2) / Math.max(40, b.x1 - b.x0), (size.h - pad * 2) / Math.max(40, b.y1 - b.y0))));
    cam = { k, x: size.w / 2 - ((b.x0 + b.x1) / 2) * k, y: size.h / 2 - ((b.y0 + b.y1) / 2) * k };
    fitted = true;
    schedule();
  }

  function zoomAt(factor: number, sx = size.w / 2, sy = size.h / 2) {
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

  function toggleExpand(index: number) {
    if (mini || queryActive) return;
    const n = slice.nodes[index];
    if (n.kind !== "document" || !slice.chunksOf[index].length) return;
    if (expanded[index]) {
      const { [index]: _, ...rest } = expanded;
      expanded = rest;
      if (mode === "chunks" && !Object.keys(rest).length) mode = "meaning";
      if (selected?.type === "node" && slice.nodes[selected.index].owner === index && selected.index !== index) selected = { type: "node", index };
    } else expanded = { ...expanded, [index]: Math.min(PORTION, slice.chunksOf[index].length) };
    rebuild(0.5);
  }

  function moreChunks(index: number) {
    expanded = { ...expanded, [index]: Math.min(slice.chunksOf[index].length, (expanded[index] ?? 0) + PORTION) };
    rebuild(0.4);
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
    mode = next;
    if (selected?.type === "edge" && !kinds[slice.edges[selected.id].kind]) selected = null;
    if (say) announce(modeNote(next));
    rebuild(0.8, true);
  }

  function cycleMode() {
    const list = availableModes;
    setMode(list[(list.indexOf(mode) + 1) % list.length]);
  }

  function leaveQuery() {
    queryOn = false;
  }

  function enterQuery() {
    if (!latest) return;
    more = 0;
    queryOn = true;
  }

  function setDepth(next: number) {
    if (next === depth) return;
    depth = next;
    more = 0;
    rebuild(0.5);
    if (selected?.type === "node" && !layout?.get(selected.index)) selected = null;
    announce(`Окружение: глубина ${next}${next === 0 ? " — только запрос и результаты" : ""}`);
  }

  function showMore() {
    if (!canMore) return;
    more++;
    rebuild(0.4);
    notice = "Продолжение того же снимка: добавлены узлы и рёбра, камера и позиции сохранены.";
  }

  function continueSlice() {
    baseSlice = { ...baseSlice, visibleDocs: undefined, truncated: false, docCount: baseSlice.totals.documents };
    notice = "Продолжение того же снимка: добавлены ещё документы, камера и позиции сохранены.";
    rebuild(0.4);
  }

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
      const reach = Math.max(8, n.r * Math.min(1.4, Math.max(0.55, Math.sqrt(cam.k))) + 5);
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
    if (target?.type === "node") tip = { text: `${label(target.index)} · ${{ document: "документ", chunk: "чанк", query: "запрос" }[slice.nodes[target.index].kind]}`, x: sx, y: sy };
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
    if (target?.type === "node") toggleExpand(target.index);
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
    else if (event.key === "Enter" && selected?.type === "node") toggleExpand(selected.index);
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

  function stub(what: string) {
    notice = `Стенд: «${what}» подключается к настоящему окну в 2.1.5; здесь показано только место и состояние действия.`;
  }

  function openFull() {
    app.view = "graph";
  }

  function runFind(event: KeyboardEvent) {
    if (event.key !== "Enter") return;
    const q = findText.trim().toLowerCase();
    const found = layout?.nodes.filter((n) => n.node.label.toLowerCase().includes(q)) ?? [];
    if (found.length) select({ type: "node", index: found[0].index });
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
  /** Вид запроса в списке: запрос первым, затем результаты по месту в выдаче. */
  const order = (n: PNode) => (n.node.kind === "query" ? -1 : (n.node.rank ?? 1e9));
  const rows = $derived.by(() => {
    void revision;
    const q = listFilter.trim().toLowerCase();
    return (layout?.nodes ?? visibleNodes().map((node) => ({ index: node.index, node, degree: 0 }) as PNode))
      .filter((n) => (!q || n.node.label.toLowerCase().includes(q) || (n.node.path ?? "").toLowerCase().includes(q)) && (!isolatesOnly || n.degree === 0))
      .sort((a, b) => order(a) - order(b) || b.degree - a.degree || a.node.label.localeCompare(b.node.label));
  });
  const layerIssues = $derived(EDGE_KINDS.filter((k) => slice.layers[k].state !== "ready"));
  const STATE_TEXT: Record<string, string> = { ready: "готов", building: "строится", partial: "частично", stale: "устарел", failed: "ошибка", cancelled: "отменён" };
  const STATUS_TEXT: Record<QueryResult["status"], string> = { ready: "", no_graph_object: "не построен", changed_since_search: "изменён", deleted_since_search: "удалён" };
  const MOTION_TEXT = { running: "Движение идёт", idle: "Раскладка в покое", stopped: "Остановлено вами", reduced: "Без движения (reduced motion)" };

  const sampleS = [0.5, 0.65, 0.8, 0.95];
  const dash = (kind: EdgeKind) => (kind === "chunk_similarity" ? "5 3" : "0");
  const edgeLine = (e: GEdge) => ({ w: widthOf(drawStrength(e, scale)), b: brightnessOf(drawStrength(e, scale)) });
  const reverseLink = (e: GEdge) => (e.kind === "markdown_link" ? slice.edges.find((x) => x.kind === "markdown_link" && x.a === e.b && x.b === e.a) : undefined);
  const edgeTitle = (e: GEdge) => {
    if (e.kind === "query_result") return `Запрос → ${label(e.b)}`;
    if (e.kind !== "markdown_link") return `${label(e.a)} — ${label(e.b)}`;
    return reverseLink(e) ? `${label(e.a)} и ${label(e.b)} ссылаются друг на друга` : `${label(e.a)} → ${label(e.b)}`;
  };
  const linesText = (e: GEdge) => (e.lines.length ? `строки ${e.lines.slice(0, 8).join(", ")}${e.lines.length > 8 ? "…" : ""}` : "строки неизвестны");
  const originText = (e: GEdge) =>
    e.kind === "markdown_link"
      ? reverseLink(e)
        ? `Взаимные явные ссылки: «${label(e.a)}» → «${label(e.b)}» (${linesText(e)}) и обратная «${label(e.b)}» → «${label(e.a)}» (${linesText(reverseLink(e)!)}). На карте одна линия со стрелкой на каждом конце; направления хранятся отдельно.`
        : `Явная Markdown-ссылка из «${label(e.a)}» в «${label(e.b)}» (${linesText(e)}); стрелка на конце, куда ведёт ссылка. Число упоминаний не усиливает пружину.`
      : e.kind === "query_result"
        ? e.scoreKind === "none"
          ? `Связь запроса «${queryText}» с результатом режима «Отношения и пути»: оценки поиска нет, линия нейтральная и не показывает силу. Типизированные пути — в списке.`
          : `Связь запроса «${queryText}» с найденным документом: ${e.scoreKind === "rerank" ? "оценка rerank" : "косинус (rerank не выполнялся)"} ${e.raw?.toFixed(2)}. Числа rerank и косинуса не сравниваются. В стенде результаты подбираются по словам названий, а не поиском bars.`
        : e.kind === "document_similarity"
          ? "Метод document-centroid-v1: косинус документных представлений по готовым векторам чанков. В стенде значение взято из сохранённого kNN, а не из centroid."
          : slice.nodes[e.a].owner === slice.nodes[e.b].owner
            ? "Косинус готовых векторов двух чанков одного документа: общий владелец, не новое межфайловое знание."
            : "Косинус готовых векторов двух чанков; kNN k=5, tau=0.5, неориентированная пара.";
  const modeCount = (m: Mode) => modeKinds(m).reduce((sum, k) => sum + counts[k], 0);
  const modeTotal = (m: Mode) => modeKinds(m).reduce((sum, k) => sum + totalCounts[k], 0);
  const modeLayer = (m: Mode) => modeKinds(m).map((k) => ({ kind: k, layer: slice.layers[k] })).find((x) => x.layer.state !== "ready" && (x.kind !== "chunk_similarity" || shownChunks > 0));
  const pct = (v: number) => `${Math.round(v * 100)}%`;

  onMount(() => {
    mini = !!host.closest(".mini");
    // Только стенд: окно проверок читает состояние панели (window.__graphDesign.full | mini).
    const debug = ((window as unknown as { __graphDesign?: Record<string, unknown> }).__graphDesign ??= {});
    debug[mini ? "mini" : "full"] = { layout: () => layout, selected: () => selected, motion: () => motion, scale: () => scale, forces: () => forces, slice: () => slice, mode: () => mode, shape: () => shape, qinfo: () => qinfo, depth: () => depth, queryOn: () => queryOn, snap: () => generalSnap, cam: () => cam };
    palette = readPalette(host);
    layout = new Layout({ seed: 7 });
    lastQueryKey = queryActive && latest ? String(latest.id) : "";
    const want = params.get("expand");
    if (want && !mini) {
      const found = slice.nodes.findIndex((n) => n.kind === "document" && n.label.includes(want));
      if (found >= 0 && slice.chunksOf[found].length) expanded = { [found]: Math.min(PORTION, slice.chunksOf[found].length) };
    }
    layout.setGraph(visibleNodes(), visibleEdges(), owner);
    const startPlan = seedPlan();
    if (startPlan) layout.place(startPlan);
    if (reduced) layout.settle(600);
    else layout.settle(40);
    revision++;
    hostObserver = new ResizeObserver(([entry]) => {
      const w = Math.floor(entry.contentRect.width);
      const h = Math.floor(entry.contentRect.height);
      if (fitted) cam = { ...cam, x: cam.x + (w - size.w) / 2, y: cam.y + (h - size.h) / 2 };
      size = { w, h };
      if (canvas) {
        const dpr = window.devicePixelRatio || 1;
        canvas.width = Math.max(1, Math.round(w * dpr));
        canvas.height = Math.max(1, Math.round(h * dpr));
      }
      if (!fitted && w > 1 && h > 1) {
        const target = pendingFocus ? layout?.get(pendingFocus.index) : null;
        if (pendingFocus && target) {
          cam = { k: pendingFocus.k, x: w / 2 - target.x * pendingFocus.k, y: h / 2 - target.y * pendingFocus.k };
          fitted = true;
        } else fit();
      }
      schedule();
    });
    hostObserver.observe(stage);
    themeObserver = new MutationObserver(() => {
      palette = readPalette(host);
      schedule();
    });
    themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["data-mode"] });
    io = new IntersectionObserver(([entry]) => {
      hidden = !entry.isIntersecting || document.hidden;
      if (!hidden) schedule();
    });
    io.observe(host);
    document.addEventListener("visibilitychange", onVisibility);
    const edgeKind = params.get("edge");
    if (edgeKind) {
      const mutualFirst = slice.edges.find((e) => e.kind === "markdown_link" && slice.edges.some((x) => x.kind === "markdown_link" && x.a === e.b && x.b === e.a));
      const found = edgeKind === "mutual" ? mutualFirst : slice.edges.find((e) => e.kind === edgeKind);
      if (found) {
        selected = { type: "edge", id: found.id };
        if (params.has("zoom")) pendingFocus = { index: found.a, k: Number(params.get("zoom")) };
      }
    }
    const pick = params.get("select");
    if (pick) {
      const found = layout.nodes.find((n) => n.node.label.includes(pick));
      if (found) {
        selected = { type: "node", index: found.index };
        if (params.has("zoom")) pendingFocus = { index: found.index, k: Number(params.get("zoom")) };
      }
    }
    schedule();
  });

  function onVisibility() {
    hidden = document.hidden;
    if (!hidden) schedule();
  }

  onDestroy(() => {
    if (raf) cancelAnimationFrame(raf);
    hostObserver?.disconnect();
    themeObserver?.disconnect();
    io?.disconnect();
    document.removeEventListener("visibilitychange", onVisibility);
    layout?.destroy();
    layout = null;
  });

  $effect(() => {
    void scale;
    schedule();
  });
  // Завершённый поиск (главный экран или глобальный слой) включает вид запроса и после ручного «Общего вида». Идущий поиск и ошибка вид не меняют:
  // результаты в окне на время поиска обнуляются, поэтому вид снимается только когда поиск не идёт, ошибки нет и сессия поиска сброшена.
  let seq = 1;
  let seenResults: unknown = null;
  $effect(() => {
    const results = app.results;
    const busy = app.searching;
    const text = app.searchedQuery.trim();
    untrack(() => {
      if (results && !busy && text && results !== seenResults) {
        seenResults = results;
        latest = { text, id: ++seq };
        queryOn = true;
        more = 0;
      } else if (!results && !busy && !app.searchError && !text && !params.has("query")) {
        latest = null;
        queryOn = false;
        seenResults = null;
      }
    });
  });
  // Вход в вид запроса и возврат к общему виду. Вход: места, камера и выбор общего вида запоминаются, раскладка строится с нуля вокруг запроса,
  // камера вписывается один раз. Возврат: общий вид получает прежние места, камеру и выбор, без нового расчёта и без вписывания.
  $effect(() => {
    const key = queryActive && latest ? String(latest.id) : "";
    untrack(() => {
      if (!layout || key === lastQueryKey) return;
      const from = lastQueryKey;
      lastQueryKey = key;
      if (key) {
        if (!from) generalSnap = { pos: layout.snapshot(), cam: { ...cam }, selected, w: size.w, h: size.h };
        selected = null;
        rebuild(0.9, true);
        announce(`Граф запроса «${queryText.length > 40 ? `${queryText.slice(0, 39)}…` : queryText}»: результатов ${resultNodes.length}, окружение — глубина ${depth}, связи — режим «${MODE_NAME[mode]}». «Общий вид» возвращает прежнюю карту`);
        return;
      }
      const snap = generalSnap;
      generalSnap = null;
      clearTimeout(fitTimer);
      if (!snap) {
        rebuild(0.9, true);
        announce("Общий вид проекта");
        return;
      }
      layout.setGraph(visibleNodes(), visibleEdges(), owner);
      layout.place(snap.pos);
      layout.cool();
      // Панель при возврате меняет высоту (исчезает полоса запроса): камера приводится к текущему размеру с сохранением центра.
      cam = { k: snap.cam.k, x: snap.cam.x + (size.w - snap.w) / 2, y: snap.cam.y + (size.h - snap.h) / 2 };
      fitted = true;
      selected = snap.selected && (snap.selected.type !== "node" || layout.get(snap.selected.index)) ? snap.selected : null;
      revision++;
      schedule();
      announce("Общий вид проекта: места узлов, камера и выбор возвращены");
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
  <h4>Связи по смыслу</h4>
  <label class="opt thr-row" title="Показывать близость не слабее выбранной силы s. Сила остальных рёбер не меняется.">
    <span>Сила от</span><input type="range" min="0.5" max="0.95" step="0.01" aria-label="Порог показа близости" bind:value={minS} oninput={onThreshold} /><b>{minS.toFixed(2)}</b>
  </label>
  <h4>Раскладка</h4>
  <label class="opt slider" title="Насколько сильно связи сближают узлы"><span>Притяжение связей</span><input type="range" min="0" max="4" step="0.1" aria-label="Притяжение по связям" bind:value={forces.link} oninput={applyForces} /><b>×{forces.link.toFixed(1)}</b></label>
  <label class="opt slider" title="Насколько узлы расталкивают друг друга"><span>Отталкивание</span><input type="range" min="0" max="4" step="0.1" aria-label="Отталкивание узлов" bind:value={forces.repel} oninput={applyForces} /><b>×{forces.repel.toFixed(1)}</b></label>
  <label class="opt slider" title="Слабое притяжение всех узлов друг к другу и к центру"><span>Общее притяжение</span><input type="range" min="0" max="4" step="0.1" aria-label="Общее притяжение узлов" bind:value={forces.gravity} oninput={applyForces} /><b>×{forces.gravity.toFixed(1)}</b></label>
  <label class="opt slider" title="Расстояние между связанными документами"><span>Длина связи</span><input type="range" min="20" max="160" step="1" aria-label="Длина связи" bind:value={forces.length} oninput={applyForces} /><b>{forces.length}</b></label>
  <button class="reset" onclick={resetForces}>По умолчанию</button>
  <p class="hint">Меняет движение узлов, а не силу связи: толщина, яркость и значения s прежние.</p>
{/snippet}

<div class="pgw" bind:this={host}>
<div class="pg" class:mini role="region" aria-label={mini ? "Мини-граф" : "Физический граф проекта"}>
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
            <b>{m === "chunks" && !shownChunks ? "—" : modeCount(m)}</b>{#if m !== "links" && modeCount(m) < modeTotal(m) && (m !== "chunks" || shownChunks)}<small>из {modeTotal(m)}</small>{/if}
            {#if issue}<span class="state {issue.layer.state}">{STATE_TEXT[issue.layer.state]}</span>{/if}
          </button>
        {/each}
      </div>
      <span class="sep"></span>
      <div class="tools">
        <button aria-label="Уменьшить масштаб" title="Уменьшить масштаб" onclick={() => zoomAt(0.8)}><Icon name="minus" size={14} width={1.8} /></button>
        <button aria-label="Увеличить масштаб" title="Увеличить масштаб" onclick={() => zoomAt(1.25)}><Icon name="plus" size={14} width={1.8} /></button>
        <button aria-label="Вписать" title="Вписать граф в панель (F)" onclick={fit}><Icon name="fit" size={14} width={1.8} /></button>
        <button aria-label={stopped ? "Запустить движение" : "Остановить движение"} aria-pressed={stopped} title={stopped ? "Запустить движение" : "Остановить движение"} onclick={() => setStopped(!stopped)} disabled={reduced}><Icon name={stopped ? "play" : "pause"} size={14} width={1.8} /></button>
        <button aria-label={view === "list" ? "Показать карту" : "Показать списком"} aria-pressed={view === "list"} title={view === "list" ? "Показать карту" : "Показать списком"} onclick={() => (view = view === "list" ? "map" : "list")}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01" /></svg>
        </button>
        <button aria-label="Настройки раскладки" aria-expanded={popover === "settings"} title="Настройки раскладки" onclick={() => (popover = popover === "settings" ? null : "settings")}><Icon name="gear" size={14} width={1.8} /></button>
        <button aria-label="Легенда и шкала силы" aria-expanded={legendOpen} title="Легенда и шкала силы" onclick={() => (legendOpen = !legendOpen)}><Icon name="info" size={14} width={1.8} /></button>
      </div>
    </div>
    {#if pendingSearch}
      <div class="banner" role="status"><span class="spin" aria-hidden="true"></span><span><b>Идёт поиск</b>{app.searchedQuery.trim() ? ` «${app.searchedQuery.trim()}»` : ""}: показан прежний вид, он обновится по ответу.</span></div>
    {/if}
    {#if failedSearch}
      <div class="banner bad" role="alert"><span><b>Поиск не удался:</b> {failedText}. Граф остался прежним.</span></div>
    {/if}
    {#if queryActive && qinfo && shape}
      <div class="qbar" role="group" aria-label="Вид запроса">
        <span class="qsq" aria-hidden="true"></span>
        <b>Запрос</b>
        <span class="qtext" title={qinfo.text}>«{qinfo.text}»</span>
        <span class="qmeta">{qinfo.results.length === 0 ? "ничего не найдено" : `${qinfo.results.length} рез.`} · «{MODE_SEARCH[qinfo.applied]}» · {SCORE_LABEL[qinfo.scoreKind]}</span>
        <div class="depth" role="radiogroup" aria-label="Окружение результатов: глубина">
          <span>Окружение</span>
          {#each [0, 1, 2] as d (d)}
            <button role="radio" aria-checked={depth === d} title={d === 0 ? "Только запрос и результаты" : d === 1 ? "Ближайшие связи результатов (по умолчанию)" : "Окружение окружения (максимум)"} onclick={() => setDepth(d)}>{d}</button>
          {/each}
        </div>
        <button class="more" onclick={leaveQuery} title="Вернуться к общему виду проекта: прежние места, камера и выбор">Общий вид</button>
      </div>
      {#if qWarnings.length}<div class="banner warn" role="status">{#each qWarnings as note (note)}<span>{note}</span>{/each}</div>{/if}
      {#if shape.truncated}
        <div class="banner" role="status">
          <span><b>{qinfo.totalKnown ? `Показано ${shape.shown.nodes} из ${shape.known.nodes} узлов` : `Показано ${shape.shown.nodes} узлов`}</b> · {shape.shown.edges}{qinfo.totalKnown ? ` из ${shape.known.edges}` : ""} связей. Причина: {shape.truncated.nodes && shape.truncated.edges ? "предел узлов и предел связей" : shape.truncated.nodes ? `предел узлов (${budget.nodes})` : `предел связей (${budget.edges})`}. Результаты поиска не вытеснены окружением.</span>
          {#if canMore}<button class="more" onclick={showMore}>Показать ещё</button>{:else}<span class="end">Достигнут максимум вида ({qinfo.budget.maxNodes} узлов, {qinfo.budget.maxEdges} связей): сузьте область или уменьшите глубину.</span>{/if}
        </div>
      {/if}
    {/if}
    {#if layerIssues.length}
      <div class="banner" role="status">
        {#each layerIssues as kind (kind)}<span><b>{KIND_TITLE[kind]}:</b> {slice.layers[kind].note}.</span>{/each}
      </div>
    {/if}
    {#if slice.truncated && !queryActive}
      <div class="banner" role="status">
        <span><b>Граф усечён:</b> показано {slice.docCount} из {slice.totals.documents} документов (бюджет среза). Связи между показанными узлами полны; остальные документы не потеряны.</span>
        <button class="more" onclick={continueSlice}>Показать ещё</button>
      </div>
    {/if}
   </div>
  {/if}

  <div class="stage" bind:this={stage}>
    {#if view === "map"}
      {#if rendererError}
        <p class="empty" role="alert">{rendererError}</p>
      {:else if slice.docCount === 0 && !queryActive}
        <div class="empty" role="status"><b>В выбранной области нет документов</b><span>Измените область в дереве или в фильтре документов.</span></div>
      {/if}
      <canvas
        bind:this={canvas}
        class:hide={slice.docCount === 0 && !queryActive}
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
      {#if slice.docCount > 0 && slice.edges.length === 0}<p class="note" role="status">У документов области нет допущенных связей: порог близости не пройден, ссылок нет.</p>{/if}
      {#if queryActive && qinfo && qinfo.results.length === 0}<p class="note qnote" role="status"><b>Ничего не найдено.</b> Порог поиска {qinfo.minScore.toFixed(2)}: ни один фрагмент его не прошёл. Узел запроса остаётся; область и порог меняются в поиске.</p>{/if}
      {#if mini && queryActive && qinfo}
        <div class="qchip" role="status"><b>Запрос</b><span title={qinfo.text}>«{qinfo.text}»</span><button class="inline" onclick={leaveQuery} title="Вернуться к общему виду: прежние места, камера и выбор">Общий вид</button></div>
      {:else if latest && !queryActive}
        <div class="qchip back" role="status"><span title={latest.text}>Запрос «{latest.text}»</span><button class="inline" onclick={enterQuery} title="Показать результаты последнего поиска в графе">{mini ? "Показать" : "Показать запрос в графе"}</button></div>
      {/if}
      {#if mini && (pendingSearch || failedSearch)}
        <div class="qchip state" class:bad={failedSearch && !pendingSearch} role={failedSearch && !pendingSearch ? "alert" : "status"}>{#if pendingSearch}<span class="spin" aria-hidden="true"></span>Идёт поиск: прежний вид{:else}Поиск не удался: вид прежний{/if}</div>
      {/if}
      {#if toast}<div class="toast" class:below={queryActive || !!latest} role="status" aria-live="polite">{toast}</div>{/if}
      <div class="status" role="status">
        <span class="dot {motion}" title={MOTION_TEXT[motion]}></span>
        {#if mini}
          {#if queryActive && qinfo && shape}<b>Запрос</b> · результатов {qinfo.results.length}{#if shape.truncated} · <b>{shape.shown.nodes}{qinfo.totalKnown ? `/${shape.known.nodes}` : ""}</b>{/if}{#if ghosts.length} · неполон{/if}{:else}<b>Документы {slice.docCount}</b>{/if} · {MODE_NAME[mode].toLowerCase()}
        {:else}
          {MOTION_TEXT[motion]} · {#if queryActive && qinfo && shape}<b>Запрос: результатов {qinfo.results.length}, окружение {Math.max(0, shape.nodes.length - 1 - resultNodes.length)}, глубина {depth}</b>{#if shape.truncated} · <b>показано {shape.shown.nodes}{qinfo.totalKnown ? ` из ${shape.known.nodes}` : ""} узлов</b>{/if}{:else}<b>Документы {slice.docCount} из {slice.totals.documents}</b>{/if}{#if shownChunks} · чанков {shownChunks} из {slice.totals.chunks}{/if}{#if isolates} · <button class="inline" onclick={() => (isolatesOnly = !isolatesOnly)} aria-pressed={isolatesOnly}>без связей: {isolates}</button>{/if}
        {/if}
      </div>
      {#if mini}
        <div class="mini-tools">
          <button aria-label="Вписать" title="Вписать" onclick={fit}><Icon name="fit" size={13} width={1.8} /></button>
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
          <button aria-label="Открыть полный граф" title="Открыть полный граф вкладкой" onclick={openFull}><Icon name="arrow-right" size={13} width={1.8} /></button>
        </div>
        {#if popover}
          <section class="pop" aria-label="Настройки раскладки">{@render forcesForm()}</section>
        {/if}
        {#if selectedNode}
          <div class="caption" role="status">
            <b title={selectedNode.label}>{selectedNode.label}</b>
            <span>{connections.length} связей</span>
            <button class="inline" onclick={() => stub("Открыть документ")}>Открыть</button>
            <button class="inline" onclick={openFull}>В графе</button>
          </div>
        {/if}
      {/if}
      {#if popover === "settings" && !mini}
        <section class="pop full" aria-label="Настройки раскладки">{@render forcesForm()}</section>
      {/if}
      {#if legendOpen && !mini}
        <section class="legend" aria-label="Легенда графа">
          <h4>Виды связей</h4>
          {#each queryActive ? ALL_KINDS : EDGE_KINDS as kind (kind)}
            <div class="row"><svg width="34" height="12" viewBox="0 0 34 12" aria-hidden="true"><line x1="1" y1="6" x2="29" y2="6" stroke={palette?.kindCss[kind] ?? "currentColor"} stroke-width="2.2" stroke-dasharray={dash(kind)} />{#if kind === "markdown_link"}<path d="M33 6l-6-3.5v7z" fill={palette?.kindCss[kind] ?? "currentColor"} />{/if}</svg>{KIND_TITLE[kind]}<small>{kind === "markdown_link" ? "стрелка на конце «куда ссылаются»" : kind === "chunk_similarity" ? "штрих, без стрелки" : kind === "query_result" ? "от запроса к результатам" : "сплошная, без стрелки"}</small></div>
          {/each}
          <p class="hint">Взаимные ссылки — одна линия со стрелками на обоих концах.</p>
          <h4>Сила связи (s)</h4>
          <p class="hint">Толщина и яркость растут с силой; шкала абсолютная, от других рёбер экрана не зависит.</p>
          <div class="scale">
            {#each sampleS as s (s)}
              {@const e = { kind: "document_similarity" as EdgeKind, s, raw: s }}
              <div><svg width="40" height="8" viewBox="0 0 40 8" aria-hidden="true"><line x1="2" y1="4" x2="38" y2="4" stroke={palette?.kindCss.document_similarity} stroke-opacity={brightnessOf(drawStrength(e, scale))} stroke-width={widthOf(drawStrength(e, scale))} /></svg><span>cos {s.toFixed(2)}</span></div>
            {/each}
          </div>
          <p class="hint">Значения разных видов не сопоставимы как мера зависимости. Близость — не доказанная зависимость.</p>
          <h4>Узлы</h4>
          <div class="row"><svg width="34" height="12" viewBox="0 0 34 12" aria-hidden="true"><circle cx="8" cy="6" r="4.5" fill="var(--muted)" /><path d="M24 0.5l5.5 5.5-5.5 5.5-5.5-5.5z" fill="var(--faint)" /></svg>документ · чанк</div>
          <div class="row"><svg width="34" height="12" viewBox="0 0 34 12" aria-hidden="true"><circle cx="8" cy="6" r="4.5" fill="none" stroke="var(--accent)" stroke-width="2" /><rect x="22" y="1" width="4" height="4" fill="var(--accent)" /></svg>выбран · закреплён</div>
          {#if queryActive}
            <div class="row"><svg width="34" height="12" viewBox="0 0 34 12" aria-hidden="true"><rect x="2" y="2" width="8" height="8" fill="var(--accent)" /><circle cx="24" cy="6" r="3.6" fill="var(--muted)" /><circle cx="24" cy="6" r="6" fill="none" stroke={palette?.kindCss.query_result} stroke-width="1.5" /></svg>запрос · результат (кольцо)</div>
            <div class="row"><svg width="34" height="12" viewBox="0 0 34 12" aria-hidden="true"><circle cx="8" cy="6" r="3.6" fill="var(--muted)" /><circle cx="8" cy="6" r="6" fill="none" stroke={palette?.kindCss.query_result} stroke-width="1.2" stroke-dasharray="2.5 2" /><circle cx="26" cy="6" r="4.5" fill="none" stroke={palette?.kindCss.query_result} stroke-width="1.6" stroke-dasharray="2.5 2.5" /></svg>связанный источник · без объекта графа</div>
            <p class="hint">Связь запроса: rerank и косинус — разные величины, вид оценки подписан в баннере и инспекторе. «Отношения и пути» — пунктир без оценки.</p>
          {/if}
          <h4>Шкала рисунка (стенд)</h4>
          <div class="seg" role="group" aria-label="Шкала рисунка">
            <button aria-pressed={scale === "project"} onclick={() => (scale = "project")}>Проект 2.1.3</button>
            <button aria-pressed={scale === "window"} onclick={() => (scale = "window")}>Окно модели 0,60–0,95</button>
          </div>
          <p class="hint">Сравнение для согласования; физика не меняется.</p>
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
                <span class="glyph {n.node.kind}"></span><span class="name" title={n.node.label}>{n.node.label}</span>{#if n.node.rank}<span class="tag{n.node.qrole === 'ghost' ? ' bad' : ''}">№{n.node.rank}{n.node.qrole === "ghost" ? " · без объекта" : ""}</span>{/if}<small>{n.node.module}</small><b>{n.degree}</b>
              </button>
            </li>
          {/each}
        </ul>
        {#if offMap.length}<p class="note" role="status">Вне карты ({offMap.length}): {offMap.map((r) => `${label(r.node)} — ${r.reason}`).join("; ")}</p>{/if}
      </div>
    {/if}
  </div>

  {#if !mini}
    <aside class="inspector" aria-label="Инспектор графа">
      {#if selectedNode}
        <header>
          <span class="kind">{selectedNode.kind === "document" ? "Документ" : selectedNode.kind === "chunk" ? "Чанк" : "Запрос"}</span>
          <h3>{selectedNode.label}</h3>
          {#if selectedNode.kind === "query"}<p class="path">Временный узел состояния графа: в Markdown и индекс ничего не записывается, в «Общем виде» его нет.</p>{:else}<p class="path">{selectedNode.path ?? "путь не определён по снимку"} · модуль {selectedNode.module}</p>{/if}
          {#if selectedNode.kind === "chunk"}<p class="path">Владелец: <button class="inline" onclick={() => select({ type: "node", index: selectedNode.owner })}>{label(selectedNode.owner)}</button></p>{/if}
        </header>
        <div class="actions">
          {#if selectedNode.kind === "query"}
            <button class="act" onclick={leaveQuery}>Общий вид</button>
          {:else}
            <button class="act" onclick={() => stub(selectedNode.kind === "document" ? "Открыть документ" : "Открыть диапазон оригинала")}><Icon name="doc" size={13} width={1.8} /> {selectedNode.kind === "document" ? "Открыть" : "Открыть диапазон"}</button>
          {/if}
          {#if selectedNode.kind === "document" && slice.chunksOf[selectedNode.index].length}
            <button class="act" onclick={() => toggleExpand(selectedNode.index)}>{expanded[selectedNode.index] ? "Свернуть чанки" : `Раскрыть чанки (${slice.chunksOf[selectedNode.index].length})`}</button>
          {/if}
          {#if selectedNode.kind !== "query"}<button class="act" onclick={() => pin(selectedNode.index)} aria-pressed={pinnedSel}>{pinnedSel ? "Открепить" : "Закрепить"}</button>{/if}
        </div>
        {#if selectedNode.kind === "query" && qinfo && shape}
          <table aria-label="Состав вида запроса">
            <tbody>
              <tr><th>Режим поиска</th><td>{MODE_SEARCH[qinfo.applied]}{qinfo.applied !== qinfo.requested ? ` (запрошен «${MODE_SEARCH[qinfo.requested]}»)` : ""}</td></tr>
              <tr><th>Оценка</th><td>{SCORE_LABEL[qinfo.scoreKind]}</td></tr>
              <tr><th>Окружение</th><td>глубина {depth}, по {qinfo.fanout} ближайших на вид</td></tr>
              <tr><th>Показано</th><td>{shape.shown.nodes}{qinfo.totalKnown ? ` из ${shape.known.nodes}` : ""} узлов · {shape.shown.edges}{qinfo.totalKnown ? ` из ${shape.known.edges}` : ""} связей</td></tr>
            </tbody>
          </table>
          <h4>Результаты · {qinfo.results.length}</h4>
          <ul class="conns res" aria-label="Результаты поиска">
            {#each qinfo.results as r (r.node)}
              <li>
                <button disabled={!r.onMap} onclick={() => select({ type: "node", index: r.node })} title={r.reason ?? ""}>
                  <span class="rk">{r.rank}</span><span class="name">{label(r.node)}</span>
                  {#if r.status !== "ready"}<span class="tag bad">{STATUS_TEXT[r.status]}</span>{:else if r.channel === "related"}<span class="tag">{r.onMap ? "связанный" : "вне карты"}</span>{/if}
                  <small>{r.score === null ? "оценки нет" : `${r.kind === "rerank" ? "rerank" : "cos"} ${r.score.toFixed(2)}`}</small>
                </button>
                {#if r.reason}<p class="why">{r.reason}</p>{/if}
              </li>
            {/each}
          </ul>
          {#if qinfo.results.length === 0}<p class="hint">Ничего не найдено: порог поиска {qinfo.minScore.toFixed(2)}.</p>{/if}
        {:else if qinfo && selectedNode.kind !== "query"}
          {@const res = qinfo.results.find((r) => r.node === selectedNode.index)}
          {#if res}
            <section class="qres" aria-label="Результат поиска">
              <h4>Результат поиска · место {res.rank} из {qinfo.results.length}</h4>
              <table>
                <tbody>
                  <tr><th>Канал</th><td>{res.channel === "primary" ? "основной результат" : res.channel === "related" ? "связанный источник" : "отношения и пути"}</td></tr>
                  <tr><th>Оценка</th><td>{res.score === null ? "нет" : `${res.score.toFixed(3)} · ${res.kind === "rerank" ? "rerank" : "косинус"}`}</td></tr>
                  <tr><th>Фрагментов</th><td>{res.chunks.length || "—"}</td></tr>
                </tbody>
              </table>
              {#if res.reason}<p class="why">{res.reason}</p>{/if}
              {#each res.chunks as c (c.label)}<p class="frag"><span class="name" title={c.label}>{c.label}</span><small>{c.score === null ? "" : c.score.toFixed(2)}</small></p>{/each}
              {#if res.chunks.length}<p class="hint">Диапазоны строк в стенде не заданы; в окне фрагменты ведут к документу со строками.</p>{/if}
              {#if res.status !== "ready"}<p class="hint">Узел остаётся в выдаче без связей: объекта в снимке графа нет.</p>{/if}
            </section>
          {:else if shape?.depthOf.has(selectedNode.index)}
            <p class="hint">Окружение результата «{label(shape.anchorOf.get(selectedNode.index) ?? 0)}»: {shape.depthOf.get(selectedNode.index)} шаг от результатов. Оценки запроса у него нет.</p>
          {/if}
        {/if}
        {#if selectedNode.kind === "document" && expanded[selectedNode.index] && expanded[selectedNode.index] < slice.chunksOf[selectedNode.index].length}
          <p class="trunc" role="status">Показано {expanded[selectedNode.index]} из {slice.chunksOf[selectedNode.index].length} чанков. <button class="inline" onclick={() => moreChunks(selectedNode.index)}>Ещё {Math.min(PORTION, slice.chunksOf[selectedNode.index].length - expanded[selectedNode.index])}</button></p>
        {/if}
        <h4>Связи · {connections.length}</h4>
        {#each ALL_KINDS as kind (kind)}
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
                    <small>{c.edge.raw === null ? "ссылка" : c.edge.raw.toFixed(2)}</small>
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
            <tr><th>{selectedEdge.kind === "query_result" ? `Оценка запроса${selectedEdge.scoreKind === "none" ? "" : selectedEdge.scoreKind === "rerank" ? " (rerank)" : " (косинус)"}` : "Исходный cosine"}</th><td>{selectedEdge.raw === null ? (selectedEdge.kind === "query_result" ? "нет" : "нет (ссылка)") : selectedEdge.raw.toFixed(3)}</td></tr>
            {#if selectedEdge.kind === "markdown_link"}<tr><th>Строки в тексте</th><td>{linesText(selectedEdge)}</td></tr>{/if}
            <tr><th>Сила s</th><td>{selectedEdge.s.toFixed(3)}{selectedEdge.kind === "markdown_link" ? " (факт наличия)" : ""}</td></tr>
            <tr><th>Толщина</th><td>{widthOf(drawStrength(selectedEdge, scale)).toFixed(2)} px</td></tr>
            <tr><th>Яркость</th><td>{pct(brightnessOf(drawStrength(selectedEdge, scale)))} цвета вида</td></tr>
            <tr><th>Притяжение q(s)</th><td>{springOf(selectedEdge.s).toFixed(3)}</td></tr>
            <tr><th>Пружина после ограничения</th><td>{springOfSelected === null ? "—" : springOfSelected.toFixed(3)}</td></tr>
          </tbody>
        </table>
        <p class="hint">{selectedEdge.kind === "markdown_link" ? "Все ссылки имеют одну силу: частота не показывает важность." : selectedEdge.kind === "query_result" ? "Связь запроса — релевантность результата, а не зависимость." : "Близость не доказывает зависимость и не заменяет ссылку."} Значения разных видов не сопоставимы.</p>
        <div class="actions">
          <button class="act" onclick={() => select({ type: "node", index: selectedEdge.a })}>Узел: {label(selectedEdge.a).slice(0, 22)}</button>
          <button class="act" onclick={() => select({ type: "node", index: selectedEdge.b })}>Узел: {label(selectedEdge.b).slice(0, 22)}</button>
          <button class="act" onclick={() => stub("Открыть исходный диапазон")}><Icon name="doc" size={13} width={1.8} /> Источник</button>
        </div>
      {:else}
        <header><span class="kind">Граф проекта</span><h3>Выберите узел или связь</h3></header>
        <p class="hint">Инспектор показывает тип, происхождение и числа связи, ведёт к источнику. Движение камеры область поиска не меняет.</p>
        <p class="hint">{slice.demo}.</p>
        <table aria-label="Состав среза">
          <tbody>
            <tr><th>Документов</th><td>{slice.docCount} из {slice.totals.documents}</td></tr>
            <tr><th>Чанков раскрыто</th><td>{shownChunks} из {slice.totals.chunks}</td></tr>
            {#each EDGE_KINDS as kind (kind)}<tr><th>{KIND_TITLE[kind]}</th><td>{kind === "chunk_similarity" && !shownChunks ? "при раскрытии" : counts[kind]} · слой {STATE_TEXT[slice.layers[kind].state]}</td></tr>{/each}
          </tbody>
        </table>
        <p class="keys"><kbd>←↑↓→</kbd> соседний узел <kbd>M</kbd> режим связей <kbd>Enter</kbd> раскрыть <kbd>Пробел</kbd> закрепить <kbd>F</kbd> вписать <kbd>Esc</kbd> снять</p>
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
  .top { grid-column: 1 / -1; grid-row: 1; min-width: 0; }
  .bar { grid-column: 1 / -1; display: flex; align-items: center; gap: 8px; padding: 4px 8px; border-bottom: 1px solid var(--line); flex-wrap: wrap; }
  .banner { grid-column: 1 / -1; display: flex; gap: 8px 14px; align-items: center; flex-wrap: wrap; padding: 4px 10px; background: var(--mid-soft); color: var(--text); border-bottom: 1px solid var(--line); font-size: 11.5px; }
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
  .mode small { color: var(--faint); font-size: 10px; }
  .mode .state { font-size: 10px; padding: 0 5px; border-radius: 8px; background: var(--mid-soft); color: var(--mid); }
  .mode .state.failed { background: var(--bad-soft); color: var(--bad); }
  .qbar { display: flex; align-items: center; flex-wrap: wrap; gap: 4px 10px; padding: 4px 10px; background: var(--accent-soft); color: var(--accent-text); border-bottom: 1px solid var(--accent); font-size: 11.5px; }
  .qbar .qtext { min-width: 0; max-width: 34ch; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 600; }
  .qbar .qmeta { color: var(--muted); flex: 1; min-width: 0; }
  .qsq { width: 9px; height: 9px; background: var(--accent); flex: none; }
  .qbar .more { border: 1px solid var(--accent); background: var(--panel); color: var(--accent-text); border-radius: 5px; padding: 1px 8px; font-size: 11.5px; }
  .depth { display: inline-flex; align-items: center; gap: 0; color: var(--muted); }
  .depth span { margin-right: 6px; }
  .depth button { width: 24px; height: 22px; padding: 0; border: 1px solid var(--line-2); background: var(--panel); color: var(--muted); font-size: 11px; margin-left: -1px; }
  .depth button:first-of-type { border-radius: 5px 0 0 5px; margin-left: 0; }
  .depth button:last-of-type { border-radius: 0 5px 5px 0; }
  .depth button[aria-checked="true"] { background: var(--accent-soft); color: var(--accent-text); border-color: var(--accent); position: relative; z-index: 1; font-weight: 600; }
  .banner.warn { background: var(--mid-soft); }
  .banner.bad { background: var(--bad-soft); color: var(--bad); }
  .banner .end { color: var(--muted); }
  .spin { width: 9px; height: 9px; border-radius: 50%; border: 2px solid var(--accent); border-top-color: transparent; animation: spin 0.8s linear infinite; flex: none; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .spin { animation: none; } }
  .qchip.back { border-style: dashed; background: var(--panel); }
  .qchip.state { top: 30px; background: var(--panel); border-color: var(--line-2); color: var(--muted); }
  .qchip.state.bad { border-color: var(--bad); color: var(--bad); }
  .qnote { top: 40px; }
  .tag { padding: 0 6px; border-radius: 8px; background: var(--accent-soft); color: var(--accent-text); font-size: 10px; white-space: nowrap; }
  .tag.bad { background: var(--bad-soft); color: var(--bad); }
  .rk { display: inline-grid; place-items: center; width: 16px; height: 16px; border-radius: 50%; background: var(--panel-2); color: var(--muted); font-size: 10px; flex: none; }
  .res li { list-style: none; }
  .res button:disabled { opacity: 0.7; cursor: default; }
  .why { margin: 0 0 4px 24px; color: var(--muted); font-size: 10.5px; line-height: 1.35; }
  .frag { display: flex; gap: 6px; margin: 2px 0; font-size: 11px; }
  .frag .name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .frag small { color: var(--muted); }
  .qres h4 { margin-top: 4px; }
  .qchip { position: absolute; left: 6px; top: 6px; display: flex; align-items: center; gap: 6px; max-width: calc(100% - 130px); padding: 2px 8px; border: 1px solid var(--accent); border-radius: 8px; background: var(--accent-soft); color: var(--accent-text); font-size: 11px; z-index: 5; }
  .qchip span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pg.mini .status { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .qchip b, .qchip button { white-space: nowrap; flex: none; }
  .toast.below { top: 32px; }
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
  .seg { display: flex; gap: 3px; }
  .seg button { flex: 1; font-size: 11px; padding: 2px 4px; border: 1px solid var(--line-2); border-radius: 5px; background: transparent; color: var(--muted); }
  .seg button[aria-pressed="true"] { background: var(--accent-soft); color: var(--accent-text); border-color: transparent; }
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
  .keys { color: var(--muted); font-size: 10.5px; line-height: 2; }
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
