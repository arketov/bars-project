// Буферы документов редактора (Т-29, НФТ-5): текст, ревизия файла и состояние записи живут здесь, а не в
// компоненте, чтобы несколько видов одного документа (панели, отдельное окно) делили один буфер.
// Устройство — docs/features/knowledge-base/editor-design.md.

import { documentRead, documentRevision, documentWrite, inTauri, type DocumentReadOnly } from "./api";

export type EditorStatus =
  | "loading"   // читается с диска
  | "clean"     // совпадает с диском
  | "dirty"     // правка есть, запись ещё не началась
  | "saving"    // идёт запись
  | "saved"     // записано
  | "error"     // запись не удалась, черновик в памяти
  | "conflict"  // диск изменился при несохранённых правках
  | "readonly"; // не UTF-8, слишком большой или файл только для чтения

/** Т-29: запись через 1 с после последнего изменения. */
export const AUTOSAVE_MS = 1000;

export const READ_ONLY_MESSAGES: Record<DocumentReadOnly, string> = {
  not_utf8: "Файл не в UTF-8: правка могла бы его испортить, документ открыт только для чтения.",
  too_large: "Файл больше 5 МБ: документ открыт только для чтения.",
  file: "Файл только для чтения.",
};

const failure = (error: unknown) => ({
  status: "error" as const,
  kind: "io",
  message: error instanceof Error ? error.message : String(error),
});

export class EditorBuffer {
  readonly project: string;
  path: string;
  text = $state("");
  /** Граница истории отмены: успешное чтение диска, даже если текст совпал. */
  historyVersion = $state(0);
  status = $state<EditorStatus>("loading");
  /** Пояснение к `error`, `conflict` и `readonly`. */
  message = $state("");
  /** Сведение, не требующее решения: «Обновлено с диска», «Ваша версия сохранена рядом». */
  notice = $state("");
  savedAt = $state<number | null>(null);
  /** Ревизия файла на диске в момент последнего чтения или записи; `null` — файла нет. */
  revision: string | null = null;
  /** Ревизия на диске, которую увидела запись при конфликте; `null` — файл удалён. */
  diskRevision: string | null = null;
  eol: "lf" | "crlf" = "lf";
  bom = false;
  views = 0;
  /** Вызывается после успешной записи: `Editors` сообщает о ней другим окнам. */
  onSaved: ((revision: string) => void) | undefined;
  #saved = "";
  #timer: ReturnType<typeof setTimeout> | undefined;
  #writing: Promise<void> | null = null;
  #generation = 0;

  constructor(project: string, path: string, seed: string) {
    this.project = project;
    this.path = path;
    this.text = seed;
    this.#saved = seed;
  }

  get dirty(): boolean {
    return this.text !== this.#saved;
  }

  /** Читает документ с диска; вне Tauri остаётся текст, переданный при создании. */
  async load(): Promise<void> {
    const generation = ++this.#generation;
    this.status = "loading";
    const read = await documentRead(this.project, this.path).catch(failure);
    if (generation !== this.#generation) return;
    if (read === null) {
      this.status = "clean";
      return;
    }
    if (read.status === "error") {
      this.status = "error";
      this.message = read.message;
      return;
    }
    this.text = read.text;
    this.historyVersion++;
    this.#saved = read.text;
    this.revision = read.revision;
    this.eol = read.eol;
    this.bom = read.bom;
    if (read.read_only) {
      this.status = "readonly";
      this.message = READ_ONLY_MESSAGES[read.read_only];
    } else {
      this.status = "clean";
      this.message = "";
    }
  }

  /** Правка пользователя: ставит автосохранение. Для документа только для чтения и при чтении игнорируется. */
  edit(text: string): void {
    if (this.status === "readonly" || this.status === "loading" || text === this.text) return;
    this.text = text;
    if (this.status === "conflict") return;
    this.status = this.dirty ? "dirty" : "clean";
    clearTimeout(this.#timer);
    if (this.status === "dirty") this.#timer = setTimeout(() => void this.save(), AUTOSAVE_MS);
  }

  /** Записывает текущий текст сейчас; одновременно идёт не больше одной записи. */
  save(): Promise<void> {
    clearTimeout(this.#timer);
    if (this.#writing) return this.#writing;
    if (!this.dirty || this.status === "readonly" || this.status === "loading" || this.status === "conflict") return Promise.resolve();
    this.#writing = this.#write().finally(() => { this.#writing = null; });
    return this.#writing;
  }

  async #write(): Promise<void> {
    const snapshot = this.text;
    this.status = "saving";
    const result = await documentWrite(this.project, this.path, snapshot, this.revision, this.eol, this.bom).catch(failure);
    if (result.status === "saved") {
      this.revision = result.revision;
      this.#saved = snapshot;
      this.savedAt = Date.now();
      this.message = "";
      this.onSaved?.(result.revision);
      // Правки пришли во время записи: следующая запись начнётся сразу после этой, не через секунду.
      this.status = this.dirty ? "dirty" : "saved";
    } else if (result.status === "conflict") {
      this.status = "conflict";
      this.diskRevision = result.revision;
      this.message = result.revision === null ? "Файл удалён с диска." : "Файл изменён на диске.";
    } else {
      this.status = "error";
      this.message = result.message;
    }
    if (this.status === "dirty") {
      this.#writing = null;
      await this.save();
    }
  }

  /** Повтор записи после ошибки. */
  retry(): Promise<void> {
    if (this.status === "error") this.status = "dirty";
    return this.save();
  }

  /**
   * Сверяет файл с диском (возвращение фокуса, открытие документа). Чистый буфер молча перечитывается;
   * буфер с правками переходит в конфликт, ни одна версия не перезаписывается.
   */
  async checkDisk(): Promise<void> {
    if (["loading", "saving", "conflict", "error"].includes(this.status) || this.#writing) return;
    const state = await documentRevision(this.project, this.path).catch(() => null);
    if (!state || state.status !== "ok" || state.revision === this.revision) return;
    // За время запроса буфер мог начать запись или конфликт: решает уже не эта проверка.
    if (this.#writing || ["loading", "saving", "conflict"].includes(this.status)) return;
    if (!this.dirty) {
      await this.load();
      if (this.status !== "error") this.notice = "Обновлено с диска";
      return;
    }
    clearTimeout(this.#timer);
    this.status = "conflict";
    this.diskRevision = state.revision;
    this.message = state.revision === null ? "Файл удалён с диска." : "Файл изменён на диске.";
  }

  /** «Оставить мою версию» и «Записать заново»: пишет буфер поверх того, что сейчас на диске. */
  keepMine(): Promise<void> {
    if (this.status !== "conflict") return Promise.resolve();
    this.revision = this.diskRevision;
    this.message = "";
    this.status = "dirty";
    return this.save();
  }

  /** «Взять версию с диска»: черновик заменяется текстом файла. */
  async takeDisk(): Promise<void> {
    clearTimeout(this.#timer);
    this.diskRevision = null;
    await this.load();
    if (this.status !== "error") this.notice = "Загружена версия с диска";
  }

  /** «Сохранить мою рядом»: буфер пишется в `<имя> (конфликт ЧЧММСС).md` в той же папке, затем берётся диск. */
  async saveBeside(): Promise<void> {
    if (this.status !== "conflict") return;
    const slash = this.path.lastIndexOf("/");
    const folder = this.path.slice(0, slash + 1);
    const name = this.path.slice(slash + 1).replace(/\.md$/i, "");
    const stamp = new Date().toTimeString().slice(0, 8).replaceAll(":", "");
    for (let n = 0; n < 10; n++) {
      const beside = `${folder}${name} (конфликт ${stamp}${n ? `-${n + 1}` : ""}).md`;
      const result = await documentWrite(this.project, beside, this.text, null, this.eol, this.bom).catch(failure);
      if (result.status === "conflict") continue;
      if (result.status === "error") {
        this.message = `Не удалось сохранить рядом: ${result.message}`;
        return;
      }
      await this.takeDisk();
      if ((this.status as EditorStatus) !== "error") this.notice = `Ваша версия сохранена рядом: ${beside.slice(slash + 1)}`;
      return;
    }
    this.message = "Не удалось подобрать свободное имя для копии рядом.";
  }

  dispose(): void {
    clearTimeout(this.#timer);
    this.#generation++;
  }
}

/** Сообщение другим окнам о записи (Т-40): окно с тем же документом сверяет диск. */
export interface SavedMessage { project: string; path: string; revision: string; origin: string }

/** Канал между окнами; в приложении — события Tauri, в тестах — подмена. */
export interface Bus {
  emit(message: SavedMessage): void;
  listen(handler: (message: SavedMessage) => void): void;
}

const SAVED_EVENT = "bars://document-saved";

function tauriBus(): Bus {
  const events = import("@tauri-apps/api/event");
  return {
    emit: (message) => void events.then(({ emit }) => emit(SAVED_EVENT, message)).catch(() => {}),
    listen: (handler) => void events.then(({ listen }) => listen<SavedMessage>(SAVED_EVENT, ({ payload }) => handler(payload))).catch(() => {}),
  };
}

export class Editors {
  #buffers = new Map<string, EditorBuffer>();
  #key = (project: string, path: string) => `${project}\n${path}`;
  #bus: Bus | undefined;
  /** Подпись окна в сообщениях: свою запись окно не принимает за чужую. */
  readonly id = Math.random().toString(36).slice(2);

  constructor(bus?: Bus) {
    this.#bus = bus ?? (typeof window !== "undefined" && inTauri() ? tauriBus() : undefined);
    this.#bus?.listen((message) => this.#remoteSaved(message));
    if (typeof window === "undefined") return;
    // Потеря фокуса и сворачивание в трей пишут сразу (Т-29); возвращение фокуса сверяет диск.
    window.addEventListener("focus", () => void this.checkAll());
    window.addEventListener("blur", () => void this.flush());
    document.addEventListener("visibilitychange", () => void (document.hidden ? this.flush() : this.checkAll()));
  }

  /** Другое окно записало документ: чистый буфер перечитывается, буфер с правками уходит в конфликт. */
  #remoteSaved(message: SavedMessage): void {
    if (message.origin === this.id) return;
    const buffer = this.get(message.project, message.path);
    if (buffer && buffer.revision !== message.revision) void buffer.checkDisk();
  }

  get(project: string, path: string): EditorBuffer | undefined {
    return this.#buffers.get(this.#key(project, path));
  }

  /** Буфер документа; новый читается с диска. `seed` показывается, пока чтение не завершено. */
  attach(project: string, path: string, seed: string): EditorBuffer {
    const key = this.#key(project, path);
    let buffer = this.#buffers.get(key);
    if (!buffer) {
      buffer = new EditorBuffer(project, path, seed);
      const attached = buffer;
      buffer.onSaved = (revision) => this.#bus?.emit({ project, path: attached.path, revision, origin: this.id });
      this.#buffers.set(key, buffer);
      if (inTauri()) void buffer.load();
      else buffer.status = "clean";
    } else if (inTauri()) void buffer.checkDisk();
    buffer.views++;
    return buffer;
  }

  /** Вид закрыт. Буфер без правок и записи удаляется; с правками остаётся и дописывается в фоне. */
  detach(buffer: EditorBuffer): void {
    buffer.views = Math.max(0, buffer.views - 1);
    if (buffer.views > 0) return;
    if (buffer.dirty) void buffer.save();
    else if (buffer.status !== "saving") {
      buffer.dispose();
      this.#buffers.delete(this.#key(buffer.project, buffer.path));
    }
  }

  /** Закрытие без записи: буфер и его правки удаляются. */
  discard(buffer: EditorBuffer): void {
    buffer.dispose();
    this.#buffers.delete(this.#key(buffer.project, buffer.path));
  }

  /** Буферы с несохранёнными правками: закрытие вкладки и окна проверяют их (НФТ-5). */
  get unsaved(): EditorBuffer[] {
    return [...this.#buffers.values()].filter((buffer) => buffer.dirty || buffer.status === "saving");
  }

  /** Записывает всё несохранённое; возвращает буферы, которые записать не удалось (конфликт, ошибка). */
  async flush(): Promise<EditorBuffer[]> {
    await Promise.all(this.unsaved.map((buffer) => buffer.save()));
    return this.unsaved;
  }

  /** Сверяет с диском открытые документы. */
  movePaths(project: string, from: string, to: string): void {
    for (const [key, buffer] of [...this.#buffers]) {
      if (buffer.project !== project) continue;
      const next = buffer.path === from ? to : buffer.path.startsWith(from + "/") ? to + buffer.path.slice(from.length) : buffer.path;
      if (next === buffer.path) continue;
      this.#buffers.delete(key); buffer.path = next;
      this.#buffers.set(this.#key(project, next), buffer);
    }
  }

  async checkAll(): Promise<void> {
    if (!inTauri()) return;
    await Promise.all([...this.#buffers.values()].filter((buffer) => buffer.views > 0).map((buffer) => buffer.checkDisk()));
  }
}

export const editors = new Editors();
