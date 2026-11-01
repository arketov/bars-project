// Действия смыслового слоя в полном графе (2.3.8): влияние, смысловой путь, контекст задачи, обзор тем, аудит связей.
// Чистая логика без DOM: запросы к `graph_query` (контракт v1 без изменений) и разбор ответов в модели для карты и инспектора.
// Правила — docs/features/graphrag/graph-actions-design.md, разделы «Влияние изменения», «Смысловой путь», «Меню «Анализ»».
// Все различия контракта v1 и его будущих схем (4.3) держатся здесь: окно получает только модели `*View`.
import { BarsError } from "../types";
import { label, limitText } from "../labels";
import { type Evidence, type GraphEdge, type GraphNode, type GraphReply, type ObjectRef } from "../graph";

export type SemanticKind = "impact" | "meaning-path" | "context" | "topics" | "audit";
export type TaskProfile = "implementation" | "investigation" | "decision";
export type Grouping = "communities" | "modules";
export interface Scope {
  modules: string[];
  files: string[];
}

export const PROFILES: { id: TaskProfile; label: string }[] = [
  { id: "implementation", label: "Реализация" },
  { id: "investigation", label: "Исследование" },
  { id: "decision", label: "Решение" },
];
/** Бюджет контекста в токенах клиента; воркер считает оценку `estimate-v1`, а не токены конкретной модели. */
export const CONTEXT_BUDGETS = [2048, 4096, 8192];
export const DEFAULT_BUDGET = 4096;
/** Глубина обхода влияния и смыслового пути; сервер может прижать значение, об этом скажет `clamped`. */
export const IMPACT_DEPTH = 3;
export const PATH_DEPTH = 4;

const SHORT_ROLE: Record<string, string> = { requirement: "Т", decision: "Р", behavior: "П", architecture: "А", constraint: "О", verification: "Пр" };
export const roleShort = (role: string): string => Object.hasOwn(SHORT_ROLE, role) ? SHORT_ROLE[role] : "?";
export const roleName = (role: string): string => label("role", role).text;

const documentRef = (node: { id: string; revision?: string }): ObjectRef => ({ kind: "document", id: node.id, revision: node.revision ?? "" });

/** Сущности документа в смысловом слое: корни влияния. У самого документа смысловых рёбер нет — зависимости соединяют сущности. */
export const ENTITY_ROOTS = 40;
export function entitiesRequest(docsPath: string): Record<string, unknown> {
  return { operation: "graph", view: "entities", scope: { modules: [], files: [docsPath] }, limit: 100 };
}
export function entityRoots(raw: GraphReply): ObjectRef[] {
  return (raw.nodes ?? []).filter((n) => n.reference.kind === "entity").map((n) => n.reference).slice(0, ENTITY_ROOTS);
}

export function impactRequest(roots: ObjectRef[], scope: Scope): Record<string, unknown> {
  return { operation: "impact", scope, roots, budgets: { max_depth: IMPACT_DEPTH }, limit: 100 };
}

export function meaningPathRequest(from: { id: string; revision?: string }, to: { id: string; revision?: string }, structural: boolean, scope: Scope): Record<string, unknown> {
  return { operation: "paths", scope, from: documentRef(from), to: documentRef(to), path_kind: structural ? "mixed" : "semantic", budgets: { max_depth: PATH_DEPTH }, max_hops: PATH_DEPTH, max_paths: 3, limit: 20 };
}

export function contextRequest(query: string, profile: TaskProfile, tokens: number, scope: Scope): Record<string, unknown> {
  return { operation: "context", scope, query: query.trim(), task_profile: profile, token_budget: tokens, limit: 50 };
}

export function topicsRequest(grouping: Grouping, scope: Scope): Record<string, unknown> {
  return { operation: "explore", analysis: "topics", whole_scope: true, grouping, scope, limit: 100 };
}

/** Аудит связей — три прохода `explore`: структура (изоляты, неразрешённые ссылки), пробелы проверок и кандидаты конфликтов. */
export function auditRequests(scope: Scope): Record<string, unknown>[] {
  return (["connectivity", "coverage", "conflict_candidates"] as const).map((analysis) => ({ operation: "explore", analysis, whole_scope: true, scope, limit: 100 }));
}

// ---------- общие помощники разбора ----------

export interface Proof {
  id: string;
  quote: string;
  /** Путь документа от корня проекта; пусто, если справочник ответа не содержит источника. */
  path: string;
  title: string;
  documentId: string;
  /** Хэш источника в снимке ответа: окно проверяет его перед выделением строк. */
  revision: string;
  firstLine: number;
  lastLine: number;
}

class Reply {
  readonly nodes = new Map<string, GraphNode>();
  readonly edges = new Map<string, GraphEdge>();
  readonly evidence = new Map<string, Evidence>();
  readonly sources = new Map<string, GraphReply["sources"][number]>();
  constructor(readonly raw: GraphReply) {
    for (const n of raw.nodes ?? []) this.nodes.set(n.reference.id, n);
    for (const n of raw.candidates ?? []) if (!this.nodes.has(n.reference.id)) this.nodes.set(n.reference.id, n);
    for (const e of raw.assertions ?? []) this.edges.set(e.id, e);
    for (const e of raw.evidence ?? []) this.evidence.set(e.id, e);
    for (const s of raw.sources ?? []) this.sources.set(s.reference.id, s);
  }

  label(ref: ObjectRef): string {
    const node = this.nodes.get(ref.id);
    if (node) return node.label;
    const source = this.sources.get(ref.id);
    return source ? source.title || source.path : `${ref.kind} ${ref.id.slice(0, 8)}`;
  }

  /** Документы, которым принадлежит объект ответа: сам документ, источники раздела/чанка/сущности или документы его оснований. */
  documents(ref: ObjectRef): string[] {
    if (ref.kind === "document") return [ref.id];
    const node = this.nodes.get(ref.id);
    if (node?.source_ids?.length) return [...new Set(node.source_ids)];
    const fromProofs = (node?.evidence ?? []).map((id) => this.evidence.get(id)?.document.id).filter((id): id is string => !!id);
    return [...new Set(fromProofs)];
  }

  proof(id: string): Proof | null {
    const ev = this.evidence.get(id);
    if (!ev) return null;
    const source = this.sources.get(ev.document.id);
    return { id, quote: ev.quote.trim(), path: source?.path ?? "", title: source?.title ?? "", documentId: ev.document.id, revision: source?.hash ?? ev.source_hash, firstLine: ev.span.first_line, lastLine: ev.span.last_line };
  }

  proofs(ids: string[], max = 4): Proof[] {
    return [...new Set(ids)].map((id) => this.proof(id)).filter((p): p is Proof => p !== null).slice(0, max);
  }

  /** Утверждение словами: «A зависит от B»; отрицание — «A не зависит от B». */
  sentence(edge: GraphEdge): string {
    const name = (id: string) => this.nodes.get(id)?.label ?? `${id.slice(0, 8)}…`;
    const word = label("predicate", edge.predicate).text;
    return `${name(edge.from)} ${edge.polarity === "negative" ? "не " : ""}${word} ${name(edge.to)}`;
  }
}

export interface ReplyNotes {
  /** Что пользователь обязан знать о полноте ответа: слой, пределы, предупреждения воркера. */
  lines: string[];
  hint?: string;
  truncated: boolean;
  /** Смысловой слой не завершён или устарел: пустой ответ не доказывает отсутствия зависимости. */
  semanticIncomplete: boolean;
}

export function replyNotes(raw: GraphReply, semantic = true): ReplyNotes {
  const lines: string[] = [];
  const layer = raw.layers?.semantic;
  const state = raw.layer_states?.semantic ?? raw.layer_state;
  const incomplete = semantic && (layer ? layer.completeness !== "complete" || layer.freshness !== "current" : state !== "ready");
  if (semantic && layer?.freshness === "stale") lines.push("Смысловой слой устарел: ответ построен по прежнему снимку.");
  if (incomplete) lines.push("Смысловой слой неполон: отсутствие результата не доказывает отсутствие связи.");
  if (raw.truncated) lines.push(`Ответ усечён: ${limitText(raw.limit_reasons) || "достигнут предел"}.`);
  else if (raw.limit_reasons?.length) lines.push(`Обход остановлен: ${limitText(raw.limit_reasons)}.`);
  for (const w of raw.warnings ?? []) if (!lines.includes(w)) lines.push(w);
  return { lines, hint: [...(raw.limit_reasons ?? []), ...(raw.warnings ?? [])].join(" · "), truncated: raw.truncated, semanticIncomplete: incomplete };
}

/** Ошибка воркера словами пользователя: «слой не построен» и «документ изменился» — не «ничего не найдено». */
export function semanticError(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);
  if (error instanceof BarsError) {
    if (error.kind === "graph_not_ready") return "Смысловой слой ещё не построен: нажмите «Обновить граф». Это не значит, что зависимостей нет.";
    if (error.kind === "stale_ref") return "Документ изменился после построения графа: обновите граф и повторите действие.";
    if (error.kind === "snapshot_expired") return "Граф обновился во время запроса: повторите действие.";
    if (error.kind === "graph_capability_unavailable") return `Воркер пока не поддерживает это действие: ${message}`;
  }
  return message;
}

// ---------- влияние ----------

export interface ImpactEntry {
  label: string;
  direct: boolean;
  /** Число шагов от документа. */
  depth: number;
  documents: string[];
  /** Путь словами: какое утверждение связывает шаг с прежним. */
  steps: string[];
  stepHints: string[];
  conditions: string[];
  proofs: Proof[];
}

export interface ImpactView {
  /** Сколько сущностей документа проверено как корни влияния. */
  entities: number;
  direct: ImpactEntry[];
  indirect: ImpactEntry[];
  /** Документ → глубина влияния (1 — прямое). Сам документ не входит. */
  documents: Map<string, number>;
  notes: ReplyNotes;
}

/** Документ без сущностей смыслового слоя: запрос влияния не отправляется, ответ говорит почему и не обещает «ничего не затронет». */
export function emptyImpact(): ImpactView {
  return { entities: 0, direct: [], indirect: [], documents: new Map(), notes: { lines: ["В документе нет сущностей смыслового слоя: явные правила ничего о нём не извлекли. Это не значит, что правка ничего не затронет."], truncated: false, semanticIncomplete: true } };
}

export function digestImpact(raw: GraphReply, rootDocument: string, entities: number): ImpactView {
  const reply = new Reply(raw);
  const entries: ImpactEntry[] = [];
  for (const item of raw.items ?? []) {
    if (item.type !== "impact") continue;
    const steps = item.path.steps.map((s) => reply.edges.get(s.edge)).filter((e): e is GraphEdge => !!e);
    entries.push({
      label: reply.label(item.node),
      direct: item.direct,
      depth: item.path.steps.length,
      documents: reply.documents(item.node).filter((id) => id !== rootDocument),
      steps: steps.map((e) => reply.sentence(e)),
      stepHints: steps.map(e => label("predicate", e.predicate).hint),
      conditions: steps.map((e) => e.qualifiers?.condition).filter((c): c is string => !!c),
      proofs: reply.proofs(item.path.evidence),
    });
  }
  entries.sort((a, b) => a.depth - b.depth || a.label.localeCompare(b.label));
  const documents = new Map<string, number>();
  for (const entry of entries) for (const id of entry.documents) if (!documents.has(id) || documents.get(id)! > entry.depth) documents.set(id, entry.depth);
  return { entities, direct: entries.filter((e) => e.direct), indirect: entries.filter((e) => !e.direct), documents, notes: replyNotes(raw) };
}

// ---------- смысловой путь ----------

export interface MeaningStep {
  sentence: string;
  /** Ход по направлению предиката утверждения. */
  forward: boolean;
  predicate: string;
  hint: string;
  conditions: string[];
  negative: boolean;
  proofs: Proof[];
}

export interface MeaningPath {
  nodes: { label: string; kind: string; documents: string[] }[];
  steps: MeaningStep[];
  complete: boolean;
}

export interface MeaningPathView {
  paths: MeaningPath[];
  /** `resolved`, `ambiguous` или `not_found`; при последних двух пути не строились. */
  resolution: string;
  candidates: string[];
  notes: ReplyNotes;
}

export function digestMeaningPaths(raw: GraphReply): MeaningPathView {
  const reply = new Reply(raw);
  const paths: MeaningPath[] = [];
  for (const item of raw.items ?? []) {
    if (item.type !== "path") continue;
    paths.push({
      complete: item.path.complete,
      nodes: item.path.nodes.map((ref) => ({ label: reply.label(ref), kind: ref.kind, documents: reply.documents(ref) })),
      steps: item.path.steps.map((s) => {
        const edge = reply.edges.get(s.edge);
        return {
          sentence: edge ? reply.sentence(edge) : "связь не найдена в справочнике ответа",
          forward: !s.reversed,
          predicate: edge ? label("predicate", edge.predicate).text : "",
          hint: edge ? label("predicate", edge.predicate).hint : "",
          conditions: edge?.qualifiers?.condition ? [edge.qualifiers.condition] : [],
          negative: edge?.polarity === "negative",
          proofs: edge ? reply.proofs(edge.evidence, 3) : [],
        };
      }),
    });
  }
  return { paths, resolution: raw.resolution ?? "resolved", candidates: (raw.candidates ?? []).map((n) => `${n.label} (${n.namespace || n.reference.kind})`), notes: replyNotes(raw) };
}

/** Документы вдоль пути в порядке первого появления: номер шага на карте. */
export function pathDocuments(path: MeaningPath): Map<string, string> {
  const result = new Map<string, string>();
  path.nodes.forEach((node, i) => {
    for (const id of node.documents) if (!result.has(id)) result.set(id, String(i + 1));
  });
  return result;
}

// ---------- контекст задачи ----------

export interface ContextGroup {
  role: string;
  label: string;
  short: string;
  proofs: Proof[];
}

export interface ContextView {
  groups: ContextGroup[];
  missing: string[];
  /** «5 из 6 ролей»: покрытые роли и все запрошенные профилем. */
  covered: number;
  total: number;
  usedBytes: number;
  maxBytes: number | null;
  /** `estimated`: бюджет — оценка по байтам, а не токены конкретной модели. */
  budgetMode: string;
  /** Документ → короткие метки ролей на карте. */
  documents: Map<string, string>;
  notes: ReplyNotes;
}

export function digestContext(raw: GraphReply): ContextView {
  const reply = new Reply(raw);
  const groups: ContextGroup[] = [];
  const documents = new Map<string, string[]>();
  for (const item of raw.items ?? []) {
    if (item.type !== "role_group") continue;
    const proofs = reply.proofs(item.evidence, 5);
    groups.push({ role: item.role, label: roleName(item.role), short: roleShort(item.role), proofs });
    for (const p of proofs) documents.set(p.documentId, [...(documents.get(p.documentId) ?? []), roleShort(item.role)]);
  }
  const missing = raw.missing_roles ?? [];
  const applied = (raw as GraphReply & { applied?: { max_bytes?: number | null } }).applied;
  return {
    groups,
    missing,
    covered: groups.length,
    total: groups.length + missing.length,
    usedBytes: raw.used_bytes ?? 0,
    maxBytes: applied?.max_bytes ?? null,
    budgetMode: raw.budget_mode,
    documents: new Map([...documents].map(([id, roles]) => [id, [...new Set(roles)].slice(0, 2).join("")])),
    notes: replyNotes(raw),
  };
}

// ---------- обзор тем ----------

export interface Topic {
  id: string;
  number: number;
  label: string;
  members: string[];
  representatives: { id: string; title: string }[];
}

export interface TopicsView {
  topics: Topic[];
  grouping: Grouping;
  /** Документ → номер темы. */
  documents: Map<string, number>;
  /** Название темы собрано из заголовков представителей, а не придумано моделью. */
  method: string;
  notes: ReplyNotes;
}

export function digestTopics(raw: GraphReply, grouping: Grouping): TopicsView {
  const reply = new Reply(raw);
  const topics: Topic[] = [];
  for (const item of raw.items ?? []) {
    if (item.type !== "topic") continue;
    topics.push({
      id: item.id,
      number: 0,
      label: item.label,
      members: item.members.map((m) => m.id),
      representatives: item.representatives.map((r) => ({ id: r.id, title: reply.label(r) })),
    });
  }
  topics.sort((a, b) => b.members.length - a.members.length || a.label.localeCompare(b.label));
  topics.forEach((t, i) => (t.number = i + 1));
  const documents = new Map<string, number>();
  for (const t of topics) for (const id of t.members) documents.set(id, t.number);
  return { topics, grouping, documents, method: raw.method, notes: replyNotes(raw, false) };
}

// ---------- аудит связей ----------

export interface AuditFinding {
  /** Документы, которых касается находка. */
  documents: string[];
  label: string;
  detail: string;
  proofs: Proof[];
}

export interface AuditSection {
  id: "isolates" | "unresolved" | "hubs" | "gaps" | "conflicts";
  title: string;
  /** Оговорка раздела: «кандидат, не вердикт» и ей подобные. */
  caution: string;
  findings: AuditFinding[];
  /** Раздел не получен: ошибка прохода или усечение. */
  error: string;
  truncated: boolean;
}

export interface AuditView {
  sections: AuditSection[];
  /** Документы всех находок: отметки на карте. */
  documents: Set<string>;
  notes: string[];
  hint: string;
}

/** Документы находки: владельцы объекта и документы его оснований — раздел или сущность в ответе может не быть описана справочником узлов. */
const owners = (reply: Reply, node: ObjectRef | undefined, proofs: Proof[]): string[] => [...new Set([...(node ? reply.documents(node) : []), ...proofs.map((p) => p.documentId)])];

const UNRESOLVED_REASON: Record<string, string> = { outside_docs_root: "ссылка ведёт за пределы документации", target_not_indexed: "цель ссылки не найдена в индексе" };

export type AuditPass = { ok: true; reply: GraphReply } | { ok: false; error: string };

/** Собирает аудит из трёх проходов `auditRequests` (по порядку) и хабов, посчитанных окном по числу ссылок на карте. */
export function digestAudit(passes: AuditPass[], hubs: { id: string; label: string; links: number }[]): AuditView {
  const [connectivity, coverage, conflicts] = passes;
  const sections: AuditSection[] = [];
  const notes: string[] = [];
  const section = (id: AuditSection["id"], title: string, caution: string, pass: AuditPass | undefined, pick: (reply: Reply) => AuditFinding[]): AuditSection => {
    if (!pass || !pass.ok) return { id, title, caution, findings: [], error: pass && !pass.ok ? pass.error : "Проход не выполнен", truncated: false };
    const reply = new Reply(pass.reply);
    const n = replyNotes(pass.reply, id === "gaps" || id === "conflicts");
    for (const line of n.lines) if (!notes.includes(line)) notes.push(line);
    return { id, title, caution, findings: pick(reply), error: "", truncated: n.truncated || (pass.reply.limit_reasons?.length ?? 0) > 0 };
  };
  const audits = (reply: Reply, kinds: string[]): { kind: string; node?: ObjectRef; evidence: string[]; detail: string }[] =>
    (reply.raw.items ?? []).flatMap((item) => (item.type === "audit" && kinds.includes(item.kind) ? [item] : []));
  sections.push(
    section("isolates", "Изоляты", "Нет индексированных Markdown-ссылок; смысловая изоляция не доказана.", connectivity, (reply) =>
      audits(reply, ["structural_isolate"]).map((a) => ({ documents: a.node ? reply.documents(a.node) : [], label: a.node ? reply.label(a.node) : "", detail: a.detail, proofs: [] })),
    ),
    section("unresolved", "Неразрешённые ссылки", "Ссылка не разрешена в индексированной документации; файл вне индекса не обязательно отсутствует.", connectivity, (reply) =>
      audits(reply, ["outside_docs_root", "target_not_indexed"]).map((a) => {
        const proofs = reply.proofs(a.evidence, 1);
        return { documents: owners(reply, a.node, proofs), label: proofs[0]?.quote ?? (a.node ? reply.label(a.node) : ""), detail: UNRESOLVED_REASON[a.kind] ?? a.detail, proofs };
      }),
    ),
    {
      id: "hubs",
      title: "Хабы",
      caution: "Считаются по числу ссылок среди документов карты; это структура, а не важность.",
      findings: hubs.map((h) => ({ documents: [h.id], label: h.label, detail: `${h.links} ссылок`, proofs: [] })),
      error: "",
      truncated: false,
    },
    section("gaps", "Кандидаты пробелов", "Кандидат, не вердикт: проверка не найдена среди отношений, извлечённых явными правилами.", coverage, (reply) =>
      audits(reply, ["verification_not_found_candidate"]).map((a) => {
        const proofs = reply.proofs(a.evidence, 1);
        return { documents: owners(reply, a.node, proofs), label: a.node ? reply.label(a.node) : (proofs[0]?.quote ?? ""), detail: a.detail, proofs };
      }),
    ),
    section("conflicts", "Кандидаты конфликтов", "Кандидат, не установленный дефект: сравните источники и условия.", conflicts, (reply) =>
      (reply.raw.items ?? []).flatMap((item) => {
        if (item.type !== "conflict") return [];
        const edges = item.assertions.map((id) => reply.edges.get(id)).filter((e): e is GraphEdge => !!e);
        const proofs = reply.proofs(item.evidence, 4);
        return [{ documents: [...new Set(proofs.map((p) => p.documentId))], label: edges[0] ? reply.sentence(edges[0]) : "Пара утверждений", detail: "Есть утверждения за и против одной связи", proofs }];
      }),
    ),
  );
  const documents = new Set<string>();
  for (const s of sections) for (const f of s.findings) for (const id of f.documents) documents.add(id);
  return { sections, documents, notes, hint: [connectivity, coverage, conflicts].flatMap(pass => pass.ok ? [...(pass.reply.limit_reasons ?? []), ...(pass.reply.assertions ?? []).map(e => e.predicate)] : []).join(" · ") };
}
