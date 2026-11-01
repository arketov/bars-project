// Подписка устанавливается до первого снимка. Поздние ответы не меняют другой проект.
import { call, inTauri, treeRead } from "./api";
import type { DiskTreeOutput } from "./api";
import type { StatusOutput, TreeOutput } from "./types";

export type PartName = "disk" | "index" | "status";
export interface Binding { subscription: string; project: string; epoch: number }
export interface Change extends Binding { revisions: number[]; recovery?: boolean }
export interface Snapshot extends Change {
  updating?: boolean;
  parts: Partial<Record<PartName, { revision: number; state: string; unchanged?: boolean; error?: string | null; value?: DiskTreeOutput | TreeOutput | StatusOutput | null }>>;
}
export interface Transport {
  listen(handler: (change: Change) => void): Promise<() => void>;
  observe(project: string | null, parts: PartName[]): Promise<Binding | null>;
  snapshot(subscription: string, known: { epoch?: number; revisions?: number[] }, force: boolean): Promise<Snapshot>;
}

function transport(): Transport {
  if (inTauri()) {
    return {
      async listen(handler) { const { listen } = await import("@tauri-apps/api/event"); return listen<Change>("bars://project-changed", ({ payload }) => handler(payload)); },
      async observe(project, parts) { const { invoke } = await import("@tauri-apps/api/core"); return invoke("window_observe", { project, parts }); },
      async snapshot(subscription, known, force) { const { invoke } = await import("@tauri-apps/api/core"); return invoke("window_snapshot", { subscription, known, force }); },
    };
  }
  // Браузерный стенд: те же части и ревизии; настоящий координатор работает в Tauri.
  let binding: Binding | null = null, names: PartName[] = [], sequence = 0;
  const hashes = new Map<PartName, string>(), revisions = [0, 0, 0];
  return {
    async listen() { return () => {}; },
    async observe(project, parts) { names = parts; hashes.clear(); return binding = project ? { subscription: String(++sequence), project, epoch: sequence } : null; },
    async snapshot(subscription, known) {
      if (!binding || binding.subscription !== subscription) throw Error("Подписка окна истекла");
      const selected = binding;
      const parts: Snapshot["parts"] = {};
      await Promise.all(names.map(async name => {
        const value = name === "disk" ? await treeRead(selected.project) : await call<TreeOutput | StatusOutput>(name === "index" ? "tree" : "status", { cwd: selected.project, project: selected.project });
        const index = ["disk", "index", "status"].indexOf(name), hash = JSON.stringify(value);
        if (hashes.get(name) !== hash) { hashes.set(name, hash); revisions[index]++; }
        parts[name] = { revision: revisions[index], state: "ready", ...(known.epoch === selected.epoch && known.revisions?.[index] === revisions[index] ? { unchanged: true } : { value }) };
      }));
      return { ...selected, revisions: [...revisions], parts };
    },
  };
}

export class ProjectUpdates {
  #transport: Transport | null = null;
  #generation = 0;
  #project: string | null = null;
  #document = false;
  #binding: Binding | null = null;
  #known: { epoch?: number; revisions?: number[] } = {};
  #required: Change | null = null;
  #pending: Promise<void> | null = null;
  #force = false;
  #unlisten: (() => void) | null = null;
  #lifecycle: Promise<unknown> = Promise.resolve();
  #bindingTask: Promise<void> = Promise.resolve();
  #retry: ReturnType<typeof setTimeout> | null = null;
  #control: ReturnType<typeof setInterval> | null = null;
  #failures = 0;
  constructor(private apply: (snapshot: Snapshot) => void, private failed: (error: string) => void, private injected?: Transport, private visible: () => boolean = () => !document.hidden) {}

  lease(action: () => Promise<void>): Promise<void> {
    const generation = this.#generation;
    const task = this.#lifecycle.catch(() => {}).then(async () => {
      if (generation === this.#generation) await action();
    });
    this.#lifecycle = task;
    return task;
  }

  ensure(project: string | null, documentWindow: boolean): Promise<void> {
    return project === this.#project && documentWindow === this.#document ? this.#bindingTask : this.bind(project, documentWindow);
  }
  reconnect(): Promise<void> {
    return this.#binding ? this.bind(this.#project, this.#document) : this.#bindingTask;
  }
  async request(project: string | null, documentWindow: boolean, force: boolean): Promise<void> {
    if (project !== this.#project || documentWindow !== this.#document) return this.bind(project, documentWindow);
    const generation = this.#generation;
    await this.#bindingTask;
    if (generation !== this.#generation) return;
    return this.refresh(force);
  }
  bind(project: string | null, documentWindow: boolean): Promise<void> {
    const generation = ++this.#generation;
    this.clearTimers(); this.#unlisten?.(); this.#unlisten = null;
    this.#project = project; this.#document = documentWindow; this.#binding = null; this.#required = null; this.#known = {}; this.#pending = null; this.#force = false;
    const api = this.#transport ??= this.injected ?? transport();
    const names: PartName[] = documentWindow ? ["disk", "index"] : ["disk", "index", "status"];
    const task = this.#lifecycle.catch(() => {}).then(async () => {
      if (generation !== this.#generation) return;
      if (project && this.visible()) {
        const stop = await api.listen(change => {
          if (generation !== this.#generation || change.project !== project || (this.#binding && change.subscription !== this.#binding.subscription)) return;
          if (change.recovery) this.failed("Слежение требует проверки; обновляем сохранённые данные");
          if (!this.#required || change.epoch > this.#required.epoch) this.#required = change;
          else if (change.epoch === this.#required.epoch) this.#required = { ...change, revisions: change.revisions.map((r, i) => Math.max(r, this.#required!.revisions[i] ?? 0)) };
          if (this.#binding) void this.refresh(false);
        });
        if (generation !== this.#generation) { stop(); return; }
        this.#unlisten = stop;
        const binding = await api.observe(project, names);
        if (generation !== this.#generation) return;
        this.#binding = binding;
        if (this.#required?.subscription !== binding?.subscription) this.#required = null;
      } else { await api.observe(null, names); }
    });
    this.#lifecycle = task;
    this.#bindingTask = task.then(async () => {
      if (generation !== this.#generation || !this.#binding) return;
      await this.refresh(false);
      if (!inTauri() && !this.injected && generation === this.#generation) this.#control = setInterval(() => void this.refresh(true), 30_000);
    }).catch(error => { if (generation === this.#generation) this.error(error); });
    return this.#bindingTask;
  }
  refresh(force: boolean): Promise<void> {
    this.#force ||= force;
    if (!this.#binding || !this.visible()) return Promise.resolve();
    if (this.#pending) return this.#pending;
    const generation = this.#generation, binding = this.#binding;
    const task = (async () => {
      do {
        const force = this.#force; this.#force = false;
        const result = await this.#transport!.snapshot(binding.subscription, this.#known, force);
        if (generation !== this.#generation || result.subscription !== binding.subscription) return;
        if (!this.#required || result.epoch >= this.#required.epoch) {
          this.#known = { epoch: result.epoch, revisions: result.revisions };
          this.apply(result); this.#failures = 0;
        }
      } while (generation === this.#generation && (this.#force || this.needsNewer()));
    })().catch(error => { if (generation === this.#generation) this.error(error); }).finally(() => { if (generation === this.#generation) this.#pending = null; });
    this.#pending = task; return task;
  }
  private needsNewer(): boolean {
    const wanted = this.#required;
    return !!wanted && (wanted.epoch > (this.#known.epoch ?? 0) || (wanted.epoch === this.#known.epoch && wanted.revisions.some((r, i) => r > (this.#known.revisions?.[i] ?? 0))));
  }
  private error(error: unknown): void {
    this.failed(error instanceof Error ? error.message : String(error));
    if (this.#retry || !this.#project || !this.visible()) return;
    const delay = [1000, 2000, 5000, 10_000, 30_000][Math.min(this.#failures++, 4)];
    this.#retry = setTimeout(() => { this.#retry = null; void this.bind(this.#project, this.#document); }, delay);
  }
  private clearTimers(): void {
    if (this.#retry) clearTimeout(this.#retry); if (this.#control) clearInterval(this.#control);
    this.#retry = null; this.#control = null;
  }
  stop(): void { void this.bind(null, this.#document); }
}
