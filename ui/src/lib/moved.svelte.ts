// Перенос путей в буферах и открытых видах; событие доставляется всем окнам.
import { inTauri } from "./api";
import { editors } from "./editors.svelte";

export interface EntryMoved { project: string; from: string; to: string }
export function movedPath(path: string, from: string, to: string): string {
  return path === from ? to : path.startsWith(from + "/") ? to + path.slice(from.length) : path;
}
class Moved {
  #listeners = new Set<(event: EntryMoved) => void>();
  #started = false;
  start() {
    if (this.#started) return;
    this.#started = true;
    if (inTauri()) void import("@tauri-apps/api/event").then(({listen}) => listen<EntryMoved>("bars://entry-moved", ({payload}) => void this.accept(payload)));
  }
  subscribe(callback: (event: EntryMoved) => void): () => void {
    this.start();
    this.#listeners.add(callback); return () => this.#listeners.delete(callback);
  }
  async accept(event: EntryMoved): Promise<void> {
    editors.movePaths(event.project, event.from, event.to);
    this.#listeners.forEach(callback => callback(event));
    await editors.checkAll();
  }
}
export const moved = new Moved();
