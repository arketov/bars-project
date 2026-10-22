// Закрытие с несохранёнными правками (НФТ-5): вопрос пользователю, закрытие вкладки и сброс правок перед выходом.

import { inTauri } from "./api";
import { editors, type EditorBuffer } from "./editors.svelte";

export type Decision = "discard" | "cancel";

class UnsavedPrompt {
  pending = $state<{ items: EditorBuffer[]; resolve: (decision: Decision) => void } | null>(null);

  /** Спрашивает, закрыть ли документы без записи. Новый вопрос отменяет прежний. */
  ask(items: EditorBuffer[]): Promise<Decision> {
    this.pending?.resolve("cancel");
    return new Promise((resolve) => {
      this.pending = { items, resolve: (decision) => { this.pending = null; resolve(decision); } };
    });
  }
}

export const unsavedPrompt = new UnsavedPrompt();

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
  return listen("bars://flush-request", async () => {
    const reply = (state: string) => invoke("flush_reply", { reply: state });
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
}
