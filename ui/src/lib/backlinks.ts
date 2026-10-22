// Входящие ссылки документа («Ссылаются на этот документ») из готового графа: два запроса `graph_query`, без нового метода воркера.
import { call } from "./api";
import type { GraphNode, GraphReply } from "./graph";

export interface Backlink {
  /** Путь документа-источника от корня проекта (как в дереве и выдаче). */
  path: string;
  title: string;
  /** Строки источника, где стоит ссылка. */
  lines: number[];
}

export interface Backlinks {
  items: Backlink[];
  /** Почему список может быть неполным или пустым не по вине документа: граф не готов, устарел, срез усечён. */
  note: string;
}

const request = (project: string, body: Record<string, unknown>) => call<GraphReply>("graph_query", { cwd: project, project, request: body });

/** Путь от корня проекта → путь от корня документации, как его понимает `scope.files`. */
export function docsRelative(path: string, docsRoot: string): string {
  const root = docsRoot.replace(/^\.\/?|\/$/g, "");
  return root && path.startsWith(`${root}/`) ? path.slice(root.length + 1) : path;
}

/** Документ-владелец узла любого вида: ссылка стоит под заголовком, поэтому её конец — раздел, а не документ. */
const ownerOf = (node: GraphNode | undefined, id: string) => (node && node.reference.kind !== "document" ? node.source_ids[0] : undefined) ?? id;

export async function loadBacklinks(project: string, path: string, docsRoot: string): Promise<Backlinks> {
  const found = await request(project, { operation: "graph", scope: { modules: [], files: [docsRelative(path, docsRoot)] }, view: "sections", limit: 500 });
  const self = found.sources.find((s) => s.path === path);
  if (!self) return { items: [], note: found.layer_state === "ready" || found.layer_state === "partial" ? "Документа нет в графе: он ещё не проиндексирован." : "Граф ещё не построен или устарел." };
  const id = self.reference.id;
  // Корни — сам документ и его разделы: ссылка может вести на раздел (`файл.md#раздел`).
  const roots = found.nodes.filter((n) => n.reference.id === id || (n.reference.kind === "section" && n.source_ids[0] === id)).map((n) => n.reference);
  const reply = await request(project, {
    operation: "graph",
    scope: { modules: [], files: [] },
    view: "sections",
    local: true,
    roots,
    direction: "incoming",
    predicates: ["links_to"],
    budgets: { max_depth: 1 },
    limit: 1000,
  });
  const nodes = new Map<string, GraphNode>([...found.nodes, ...reply.nodes].map((n) => [n.reference.id, n]));
  const sources = new Map(reply.sources.map((s) => [s.reference.id, s]));
  const evidence = new Map(reply.evidence.map((e) => [e.id, e]));
  const byPath = new Map<string, Backlink>();
  for (const item of reply.items) {
    if (item.type !== "edge" || item.edge.predicate !== "links_to") continue;
    const to = ownerOf(nodes.get(item.edge.to), item.edge.to);
    const from = ownerOf(nodes.get(item.edge.from), item.edge.from);
    if (to !== id || from === id) continue;
    const source = sources.get(from);
    if (!source) continue;
    const entry = byPath.get(source.path) ?? { path: source.path, title: source.title, lines: [] };
    for (const proof of item.edge.evidence) {
      const line = evidence.get(proof)?.span.first_line;
      if (line !== undefined && !entry.lines.includes(line)) entry.lines.push(line);
    }
    entry.lines.sort((a, b) => a - b);
    byPath.set(source.path, entry);
  }
  const notes: string[] = [];
  if (reply.layer_state === "stale") notes.push("Граф устарел: документы изменились после постройки.");
  else if (reply.layer_state !== "ready" && reply.layer_state !== "partial") notes.push("Граф ещё не готов.");
  if (reply.truncated) notes.push("Список усечён по пределам запроса.");
  return { items: [...byPath.values()].sort((a, b) => a.path.localeCompare(b.path)), note: notes.join(" ") };
}
