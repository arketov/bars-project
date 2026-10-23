// Хранилище цветов и иконок проекта (Т-38): читает Index.md модулей и локальный TOML через Tauri,
// держит одну копию на окно и перечитывает её по событию из другого окна.
// Вне Tauri (просмотр в браузере) значения живут в памяти страницы.

import { app } from "./app.svelte";
import * as api from "./api";
import type { AppearanceReply } from "./api";
import { canStyleFolder, emptyAppearance, validColor, validIcon, type AppearanceData } from "./appearance";

// Тесты подменяют `./api` целиком; без нужной функции работаем как в браузере.
const tauri = (): boolean => { try { return api.inTauri(); } catch { return false; } };

const EVENT = "bars://appearance-changed";

class Appearance {
  data = $state.raw<AppearanceData>(emptyAppearance());
  project = $state<string | null>(null);
  /** Текст ошибки чтения: файл не разбирается, цвета показываются по умолчанию. */
  error = $state("");
  #memory = new Map<string, AppearanceData>();
  #listening = false;

  #accept(project: string, reply: AppearanceReply | null) {
    if (project !== this.project) return;
    if (reply?.status === "ok") {
      this.data = { folders: reply.folders, documents: reply.documents };
      this.error = "";
    } else if (reply) {
      this.data = emptyAppearance();
      this.error = reply.message;
    }
  }

  async load(project: string | null) {
    this.project = project;
    this.data = emptyAppearance();
    this.error = "";
    if (!project) return;
    if (!tauri()) {
      this.data = this.#memory.get(project) ?? emptyAppearance();
      return;
    }
    this.#listen();
    try {
      this.#accept(project, await api.appearanceRead(project));
    } catch (error) {
      if (project === this.project) this.error = String(error);
    }
  }

  #listen() {
    if (this.#listening) return;
    this.#listening = true;
    void import("@tauri-apps/api/event")
      .then(({ listen }) => listen<{ project: string }>(EVENT, ({ payload }) => { if (payload.project === this.project) void this.load(this.project); }))
      .catch(() => { this.#listening = false; });
  }

  /** Записывает цвет и иконку одной записи; возвращает текст ошибки или `null`. */
  async set(kind: "folder" | "document", key: string, color: string | null, icon: string | null): Promise<string | null> {
    const project = this.project;
    if (!project) return "Проект не выбран";
    if (color !== null && !validColor(color)) return "Такого цвета нет в палитре окна";
    if (icon !== null && !validIcon(icon)) return "Такой иконки нет во встроенном наборе";
    if (kind === "document" && icon !== null) return "У документа иконки нет, только цвет";
    if (kind === "folder" && !canStyleFolder(key)) return "Цвет и иконка выбираются только у модуля и его подпапок второго уровня";
    if (!tauri()) {
      const next = structuredClone($state.snapshot(this.data)) as AppearanceData;
      const entries = kind === "folder" ? next.folders : next.documents;
      if (color === null && icon === null) delete entries[key];
      else entries[key] = { color, icon };
      this.#memory.set(project, next);
      this.data = next;
      return null;
    }
    try {
      const reply = await api.appearanceSet(project, kind, key, color, icon);
      if (reply?.status === "error") return reply.message;
      this.#accept(project, reply);
      return null;
    } catch (error) {
      return String(error);
    }
  }
}

export const appearance = new Appearance();

// Оформление проекта перечитывается при его смене; хранилище одно на окно.
$effect.root(() => {
  $effect(() => {
    const project = app.current;
    app.tree; // Изменения Index.md с диска обновляют оформление модулей.
    void appearance.load(project);
  });
});
