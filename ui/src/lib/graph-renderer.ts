import cytoscape, {
  type Core,
  type Layouts,
  type Position,
  type StylesheetJson,
} from "cytoscape";
import type { GraphReply, GraphNode } from "./graph";

interface Saved {
  positions: Map<string, Position>;
  pinned: Set<string>;
  pan: Position;
  zoom: number;
}
const cameras = new Map<string, Saved>();
export class GraphRenderer {
  readonly cy: Core;
  #layout: Layouts | null = null;
  #layoutTimer: ReturnType<typeof setTimeout> | undefined;
  #resize: ResizeObserver | null = null;
  #theme: MutationObserver | null = null;
  #saved: Saved;
  #count = 0;
  constructor(
    container: HTMLElement,
    key: string,
    onSelect: (id: string, edge: boolean) => void,
  ) {
    this.#saved = cameras.get(key) ?? {
      positions: new Map(),
      pinned: new Set(),
      pan: { x: 0, y: 0 },
      zoom: 1,
    };
    cameras.set(key, this.#saved);
    while (cameras.size > 16) cameras.delete(cameras.keys().next().value!);
    this.cy = cytoscape({
      container,
      elements: [],
      layout: { name: "preset" },
      wheelSensitivity: 0.2,
      minZoom: 0.08,
      maxZoom: 5,
      style: [],
    });
    this.cy.on("tap", "node", (event) => onSelect(event.target.id(), false));
    this.cy.on("tap", "edge", (event) => onSelect(event.target.id(), true));
    this.#resize = new ResizeObserver(() => {
      this.cy.resize();
      if (this.cy.nodes().length) this.cy.fit(undefined, 40);
    });
    this.#resize.observe(container);
    this.#theme = new MutationObserver(() => this.style(container));
    this.#theme.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-mode"],
    });
    this.style(container);
  }
  private style(container: HTMLElement) {
    const css = getComputedStyle(container),
      color = (key: string, fallback: string) =>
        css.getPropertyValue(key).trim() || fallback;
    this.cy.style([
      {
        selector: "node",
        style: {
          label: "data(label)",
          "font-size": 11,
          "text-wrap": "ellipsis",
          "text-max-width": "140px",
          "text-valign": "bottom",
          "text-margin-y": 6,
          color: color("--text", "#312b34"),
          "background-color": color("--accent", "#dc3577"),
          width: 22,
          height: 22,
        },
      },
      {
        selector: 'node[kind="document"]',
        style: { shape: "round-rectangle" },
      },
      { selector: 'node[kind="section"]', style: { shape: "rectangle" } },
      { selector: 'node[kind="entity"]', style: { shape: "diamond" } },
      {
        selector: "edge",
        style: {
          width: 1.3,
          "line-color": color("--muted", "#999"),
          "target-arrow-color": color("--muted", "#999"),
          "target-arrow-shape": "triangle",
          "curve-style": "bezier",
          "arrow-scale": 0.65,
        },
      },
      {
        selector: 'edge[predicate="similar_to"]',
        style: {
          "line-style": "dashed",
          "target-arrow-shape": "none",
          opacity: 0.5,
        },
      },
      {
        selector: 'edge[polarity="negative"]',
        style: {
          "line-style": "dotted",
          "line-color": color("--bad", "#b8364e"),
        },
      },
      {
        selector: ".highlight",
        style: {
          "line-color": color("--accent", "#dc3577"),
          "target-arrow-color": color("--accent", "#dc3577"),
          width: 4,
        },
      },
      {
        selector: ".picked",
        style: {
          "border-width": 3,
          "border-color": color("--text", "#312b34"),
        },
      },
      {
        selector: ":parent",
        style: {
          "background-opacity": 0.05,
          "border-color": color("--line", "#ddd"),
          "border-width": 1,
          padding: "22px",
          "text-valign": "top",
        },
      },
    ] as StylesheetJson);
  }
  private remember() {
    this.cy.nodes().forEach((n) => {
      this.#saved.positions.set(n.id(), { ...n.position() });
    });
    this.#saved.pan = { ...this.cy.pan() };
    this.#saved.zoom = this.cy.zoom();
  }
  set(
    reply: GraphReply,
    selected: string | null,
    grouped: boolean,
    highlight: string[] = [],
  ) {
    if (this.#count) this.remember();
    this.stop();
    this.cy.elements().remove();
    const nodes = reply.nodes,
      ids = new Set(nodes.map((n) => n.reference.id));
    for (const id of this.#saved.positions.keys())
      if (!ids.has(id)) {
        this.#saved.positions.delete(id);
        this.#saved.pinned.delete(id);
      }
    const elements: cytoscape.ElementDefinition[] = [];
    const groups = new Set<string>();
    const parent = (node: GraphNode) =>
      grouped &&
      node.reference.kind !== "document" &&
      node.source_ids.length === 1
        ? "group:" + node.source_ids[0]
        : undefined;
    for (const node of nodes) {
      const group = parent(node);
      if (group && !groups.has(group)) {
        groups.add(group);
        elements.push({
          data: {
            id: group,
            label:
              reply.sources.find((s) => s.reference.id === node.source_ids[0])
                ?.title ?? "Документ",
          },
        });
      }
    }
    for (const [i, node] of nodes.entries())
      elements.push({
        data: {
          id: node.reference.id,
          label: node.label,
          kind: node.reference.kind,
          parent: parent(node),
        },
        position: this.#saved.positions.get(node.reference.id) ?? {
          x: Math.cos(i * 2.4) * Math.sqrt(i + 1) * 40,
          y: Math.sin(i * 2.4) * Math.sqrt(i + 1) * 40,
        },
        locked: this.#saved.pinned.has(node.reference.id),
        classes: node.reference.id === selected ? "picked" : "",
      });
    for (const edge of reply.assertions)
      if (ids.has(edge.from) && ids.has(edge.to))
        elements.push({
          data: {
            id: edge.id,
            source: edge.from,
            target: edge.to,
            predicate: edge.predicate,
            polarity: edge.polarity,
          },
          classes: highlight.includes(edge.id) ? "highlight" : "",
        });
    this.cy.add(elements);
    this.cy.layout({ name: "preset", fit: false }).run();
    if (this.#count) {
      this.cy.zoom(this.#saved.zoom);
      this.cy.pan(this.#saved.pan);
    } else this.cy.fit(undefined, 40);
    this.#count = nodes.length;
  }
  fit() {
    this.cy.fit(undefined, 40);
  }
  zoom(factor: number) {
    this.cy.zoom({
      level: this.cy.zoom() * factor,
      renderedPosition: { x: this.cy.width() / 2, y: this.cy.height() / 2 },
    });
  }
  pin(id: string) {
    const node = this.cy.$id(id);
    if (node.locked()) {
      node.unlock();
      this.#saved.pinned.delete(id);
    } else {
      node.lock();
      this.#saved.pinned.add(id);
    }
    this.remember();
  }
  pinned(id: string) {
    return this.#saved.pinned.has(id);
  }
  layout() {
    this.stop();
    this.#layout =
      this.cy.nodes().length > 500
        ? this.cy.layout({ name: "grid", fit: false })
        : this.cy.layout({
            name: "cose",
            animate: !matchMedia("(prefers-reduced-motion: reduce)").matches,
            randomize: false,
            numIter: 120,
            fit: false,
          });
    this.#layoutTimer = setTimeout(() => this.stop(), 600);
    this.#layout.run();
  }
  stop() {
    if (this.#layoutTimer) clearTimeout(this.#layoutTimer);
    this.#layoutTimer = undefined;
    this.#layout?.stop();
    this.#layout = null;
  }
  destroy() {
    if (this.cy.destroyed()) return;
    this.remember();
    this.stop();
    this.#resize?.disconnect();
    this.#theme?.disconnect();
    this.cy.removeAllListeners();
    this.cy.destroy();
  }
}
