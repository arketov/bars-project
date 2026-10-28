// Закрытие с несохранёнными правками (НФТ-5): вопрос пользователю, закрытие вкладки и сброс правок перед выходом.

import { inTauri } from "./api";
import { editors, type EditorBuffer } from "./editors.svelte";

export type Decision = "discard" | "cancel";

class UnsavedPrompt {
  pending = $state<{ items: EditorBuffer[]; allowBeside: boolean; busy: boolean; resolve: (decision: Decision) => void } | null>(null);

  /** Спрашивает, закрыть ли документы без записи. Новый вопрос отменяет прежний. */
  ask(items: EditorBuffer[], allowBeside = false): Promise<Decision> {
    this.pending?.resolve("cancel");
    return new Promise((resolve) => {
      this.pending = { items, allowBeside, busy: false, resolve: (decision) => { this.pending = null; resolve(decision); } };
    });
  }

  async saveBeside(): Promise<void> {
    const pending = this.pending;
    if (!pending || pending.busy || !pending.allowBeside) return;
    pending.busy = true;
    for (const buffer of pending.items.filter(b => b.dirty)) {
      await buffer.saveBeside();
      if (buffer.dirty) { pending.busy = false; return; }
    }
    pending.busy = false;
    if (this.pending === pending) pending.resolve("discard");
  }
}

export const unsavedPrompt = new UnsavedPrompt();

/** Переход не отцепляет документы, пока не записаны правки или не принято решение. */
export async function confirmProjectLeave(project: string): Promise<boolean> {
  const rest = await editors.flush(project);
  if (!rest.length) return true;
  const beside = rest.every(b => b.status === "conflict" && b.diskRevision !== null);
  if (await unsavedPrompt.ask(rest, beside) === "cancel") return false;
  rest.forEach(buffer => editors.discard(buffer));
  return true;
}

/** Можно ли закрыть вкладку документа: правка сначала записывается; при отказе записи решает пользователь. */
export async function confirmClose(project: string | null, path: string): Promise<boolean> {
  const buffer = project ? editors.get(project, path) : undefined;
  // Другой вид держит тот же буфер: закрытие этого вида ничего не теряет.
  if (!buffer || buffer.views > 1) return true;
  await buffer.save();
  if (!buffer.dirty && buffer.status !== "saving") return true;
  if ((await unsavedPrompt.ask([buffer])) === "cancel") return false;
  editors.discard(buffer);
  return true;
}

/** Просьба процесса выйти (трей, перезапуск, установщик): окно записывает правки и при отказе спрашивает. */
export async function startFlushListener(): Promise<() => void> {
  if (!inTauri()) return () => {};
  const [{ listen }, { invoke }] = await Promise.all([import("@tauri-apps/api/event"), import("@tauri-apps/api/core")]);
  const moveStop = await listen<string>("bars://move-flush-request", async ({ payload }) => {
    const rest = await editors.flush(payload);
    await invoke("flush_reply", {reply: rest.length ? "cancel" : "clean"});
  });
  const stop = await listen("bars://flush-request", async () => {
    const reply = (state: string) => invoke("flush_reply", { reply: state });
    // Запись может идти дольше первых 3 с ожидания процесса: «writing» просит ждать её окончания (OQ-174).
    if (editors.unsaved.length > 0) await reply("writing");
    const rest = await editors.flush();
    if (rest.length === 0) {
      await reply("clean");
      return;
    }
    await reply("asking");
    const decision = await unsavedPrompt.ask(rest);
    if (decision === "discard") rest.forEach((buffer) => editors.discard(buffer));
    await reply(decision === "discard" ? "proceed" : "cancel");
  });
  // Выход отменён, потому что запись не кончилась за минуту: правка осталась на экране.
  const stillWriting = await listen("bars://flush-still-writing", () => {
    void import("./app.svelte").then(({ app }) => app.flash("Документ ещё записывается", true));
  });
  return () => { stop(); moveStop(); stillWriting(); };
}
