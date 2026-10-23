// Состояние экрана «Аудиты». Всё знает воркер: окно хранит, какие виды раскрыты и что показывать
// из дублей, и отбрасывает поздние ответы после смены проекта (docs/features/knowledge-base/audits.md).

import { call } from "./api";
import {
  KIND_LABEL,
  isBusy,
  newTotal,
  type AuditFindingsPage,
  type AuditKind,
  type AuditList,
  type AuditRunAnswer,
  type AuditSummary,
  type AuditFinding,
  type FindingGroup,
  type ScheduleMode,
} from "./audits";
import type { Flash } from "./new-topics.svelte";

const message = (error: unknown): string => (error instanceof Error ? error.message : String(error));

export interface KindFindings {
  items: AuditFindingsPage["items"];
  total: number;
  next: number | null;
  loading: boolean;
  error: string | null;
  /** Полный запуск, из которого взяты находки: при смене список перечитывается с начала. */
  runId: number | null;
}

const emptyFindings = (): KindFindings => ({ items: [], total: 0, next: null, loading: false, error: null, runId: null });

export class AuditsStore {
  project = $state<string | null>(null);
  list = $state<AuditList | null>(null);
  loading = $state(false);
  error = $state<string | null>(null);
  /** Какие виды раскрыты: список находок виден под строкой вида. */
  open = $state<Record<string, boolean>>({});
  findings = $state<Record<string, KindFindings>>({});
  /** «Перекрытие содержания»: смысловые и слабые частичные повторы. */
  showOverlap = $state(false);
  showIgnored = $state(false);
  /** Идёт запрос запуска, остановки или смены расписания. */
  busy = $state(false);

  #flash: Flash;
  #version = 0;
  #seq = 0;
  #findingSeq: Record<string, number> = {};
  #notice: (project: string, text: string) => void;
  /** Сумма новых находок при прошлом чтении: уведомление только при приросте (A-04). */
  #seenNew: number | null = null;
  #seenResults: Record<string, number | undefined> = {};

  constructor(flash: Flash = () => {}, notice: (project: string, text: string) => void = () => {}) {
    this.#flash = flash;
    this.#notice = notice;
  }

  setFlash(flash: Flash): void {
    this.#flash = flash;
  }

  /** Новых находок во всех видах: число рядом со строкой дерева; `null` — списка нет. */
  get newCount(): number | null {
    return this.list && !this.error ? newTotal(this.list) : null;
  }

  summary(kind: AuditKind): AuditSummary | null {
    return this.list?.audits.find((audit) => audit.kind === kind) ?? null;
  }

  get anyBusy(): boolean {
    return !!this.list?.audits.some(isBusy);
  }

  bind(project: string | null): void {
    if (project === this.project) return;
    this.#version++;
    this.project = project;
    this.list = null;
    this.error = null;
    this.loading = false;
    this.open = {};
    this.findings = {};
    this.showOverlap = false;
    this.showIgnored = false;
    this.#findingSeq = {};
    this.busy = false;
    this.#seenNew = null;
    this.#seenResults = {};
  }

  /** Читает состояния видов. `quiet` — фоновое обновление без индикатора и без показа ошибки поверх данных. */
  async load(quiet = false): Promise<void> {
    const project = this.project;
    if (!project) return;
    const version = this.#version;
    const seq = ++this.#seq;
    if (!quiet) this.loading = true;
    try {
      const list = await call<AuditList>("audit_list", { project });
      if (version !== this.#version || seq !== this.#seq) return;
      if (!Array.isArray(list?.audits)) throw new Error("ответ воркера об аудитах не разобран");
      this.list = list;
      this.error = null;
      this.#notify(list);
      this.#refreshOpen(list);
    } catch (error) {
      if (version !== this.#version || seq !== this.#seq) return;
      if (!quiet || this.list === null) this.error = message(error);
    } finally {
      if (version === this.#version && seq === this.#seq) this.loading = false;
    }
  }

  /** Уведомление приходит, когда число новых находок выросло, а не после каждого запуска. */
  #notify(list: AuditList): void {
    const total = newTotal(list);
    const fresh = list.audits.filter(audit => audit.new_count > 0 && audit.result_run_id !== this.#seenResults[audit.kind]).map(audit => KIND_LABEL[audit.kind]);
    if (this.#seenNew !== null && total > this.#seenNew && fresh.length) {
      this.#flash(`Аудит: новые находки (${total}) — ${fresh.join(", ")}`);
      if (this.project) this.#notice(this.project, `Новые находки (${total}) — ${fresh.join(", ")}`);
    }
    this.#seenNew = total;
    this.#seenResults = Object.fromEntries(list.audits.map(audit => [audit.kind, audit.result_run_id]));
  }

  /** Раскрытый вид перечитывается, когда у него появился новый полный результат. */
  #refreshOpen(after: AuditList): void {
    for (const audit of after.audits) {
      if (!this.open[audit.kind]) continue;
      const loaded = this.findings[audit.kind]?.runId ?? null;
      if (audit.result_run_id !== undefined && audit.result_run_id !== loaded && !this.findings[audit.kind]?.loading) {
        void this.loadFindings(audit.kind, true);
      }
    }
  }

  group(kind: AuditKind): FindingGroup | null {
    return kind === "duplicates" && !this.showOverlap ? "main" : null;
  }

  async toggle(kind: AuditKind): Promise<void> {
    const next = !this.open[kind];
    this.open = { ...this.open, [kind]: next };
    if (next && !this.findings[kind]?.items.length) await this.loadFindings(kind, true);
  }

  async setOverlap(show: boolean): Promise<void> {
    this.showOverlap = show;
    if (this.open.duplicates) await this.loadFindings("duplicates", true);
  }

  async setShowIgnored(show: boolean): Promise<void> {
    this.showIgnored = show;
    await Promise.all(Object.keys(this.open).filter(k => this.open[k]).map(k => this.loadFindings(k as AuditKind, true)));
  }

  async ignore(kind: AuditKind, finding: AuditFinding): Promise<void> {
    const project = this.project;
    if (!project || this.busy) return;
    const version = this.#version;
    this.busy = true;
    try {
      await call("audit_ignore", { project, kind, fingerprint: finding.fingerprint, ignored: !finding.ignored });
      if (version !== this.#version) return;
      await this.load(true);
      await this.loadFindings(kind, true);
    } catch (error) {
      if (version === this.#version) this.#flash(message(error), true);
    } finally {
      if (version === this.#version) this.busy = false;
    }
  }

  /** Страница находок. `reset` — с начала; иначе следующая по курсору. */
  async loadFindings(kind: AuditKind, reset: boolean): Promise<void> {
    const project = this.project;
    if (!project) return;
    const version = this.#version;
    const current = this.findings[kind] ?? emptyFindings();
    if (!reset && current.next === null) return;
    if (!reset && current.loading) return;
    const seq = (this.#findingSeq[kind] ?? 0) + 1;
    this.#findingSeq[kind] = seq;
    this.findings = { ...this.findings, [kind]: { ...(reset ? emptyFindings() : current), loading: true } };
    try {
      const group = this.group(kind);
      const page = await call<AuditFindingsPage>("audit_findings", {
        project,
        kind,
        include_gone: true,
        include_ignored: this.showIgnored,
        ...(group ? { group } : {}),
        ...(!reset && current.next !== null ? { cursor: current.next } : {}),
      });
      if (version !== this.#version || seq !== this.#findingSeq[kind]) return;
      // Переключатель мог смениться, пока шёл запрос: устаревшую страницу не берём.
      if (this.group(kind) !== group) return;
      if (!reset && current.runId !== (page.run_id ?? null)) { await this.loadFindings(kind, true); return; }
      const items = reset ? page.items : [...current.items, ...page.items];
      this.findings = {
        ...this.findings,
        [kind]: { items, total: page.total, next: page.next ?? null, loading: false, error: null, runId: page.run_id ?? null },
      };
    } catch (error) {
      if (version !== this.#version || seq !== this.#findingSeq[kind]) return;
      this.findings = { ...this.findings, [kind]: { ...(reset ? emptyFindings() : current), loading: false, error: message(error) } };
    }
  }

  async run(kind: AuditKind): Promise<void> {
    const project = this.project;
    if (!project || this.busy) return;
    const version = this.#version;
    this.busy = true;
    try {
      const answer = await call<AuditRunAnswer>("audit_run", { project, kind });
      if (version !== this.#version) return;
      if (answer.queued) this.#flash(`«${KIND_LABEL[kind]}» ждёт: в проекте идёт другой аудит`);
      await this.load(true);
    } catch (error) {
      if (version === this.#version) this.#flash(message(error), true);
    } finally {
      if (version === this.#version) this.busy = false;
    }
  }

  async cancel(kind: AuditKind): Promise<void> {
    const project = this.project;
    if (!project) return;
    const version = this.#version;
    try {
      await call("audit_cancel", { project, kind });
      if (version === this.#version) await this.load(true);
    } catch (error) {
      if (version === this.#version) this.#flash(message(error), true);
    }
  }

  async setSchedule(kind: AuditKind, mode: ScheduleMode): Promise<void> {
    const project = this.project;
    if (!project || this.busy) return;
    const version = this.#version;
    this.busy = true;
    try {
      await call("audit_schedule", { project, kind, mode });
      if (version === this.#version) await this.load(true);
    } catch (error) {
      if (version === this.#version) this.#flash(message(error), true);
    } finally {
      if (version === this.#version) this.busy = false;
    }
  }
}
