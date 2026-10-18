// Состояние экрана настроек: черновик, сохранённое, модели, клиенты. Запись идёт через воркер
// (`settings_save`), ключи — в хранилище ОС (`secret_set`); окно файлов не пишет (docs/architecture/secrets.md).

import { app } from "./app.svelte";
import { call } from "./api";
import { clone, isDirty, needsReindex, parseFingerprint, validate } from "./settings-logic";
import type {
  ClientsStatusOutput,
  AutostartOutput,
  ClientStatus,
  InstallClientName,
  InstructionOutput,
  ModelsInfoOutput,
  SettingsDoc,
  SettingsOutput,
} from "./types";

/** Имя записи в хранилище ОС по умолчанию, если пользователь задаёт ключ впервые. */
export const DEFAULT_KEY_NAMES = { embed: "bars-embed", rerank: "bars-rerank" } as const;

const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

class SettingsState {
  file = $state("");
  warnings = $state<string[]>([]);
  saved = $state<SettingsDoc | null>(null);
  draft = $state<SettingsDoc | null>(null);
  get models(): ModelsInfoOutput | null { return app.models; }
  clients = $state<ClientStatus[]>([]);
  /** Автозапуск при входе в систему: состояние ОС, а не settings.toml; `null` — не прочитано. */
  autostart = $state<AutostartOutput | null>(null);
  instruction = $state<InstructionOutput | null>(null);
  /** Есть ли запись с этим именем в хранилище ОС. */
  secrets = $state<Record<string, boolean>>({});
  saving = $state(false);
  loadError = $state<string | null>(null);
  busy = $state<string | null>(null);

  get dirty(): boolean {
    return !!this.saved && !!this.draft && isDirty(this.saved, this.draft);
  }

  get errors(): Record<string, string> {
    return this.draft ? validate(this.draft) : {};
  }

  /** Сохранение сменит модель индекса: проектам понадобится переиндексация (Т-25). */
  get willReindex(): boolean {
    return !!this.saved && !!this.draft && needsReindex(this.saved, this.draft);
  }

  async load(): Promise<void> {
    try {
      const output = await call<SettingsOutput>("settings_get");
      this.file = output.file;
      this.warnings = output.warnings;
      this.saved = clone(output.settings);
      this.draft = clone(output.settings);
      this.loadError = null;
    } catch (error) {
      this.loadError = error instanceof Error ? error.message : String(error);
    }
    await Promise.all([this.loadModels(), this.loadClients(), this.loadSecrets(), this.loadAutostart()]);
  }

  async loadModels(): Promise<void> {
    try {
      await app.loadModels();
    } catch {
      // Последний успешный снимок остаётся общим с центром уведомлений.
    }
  }

  async loadClients(): Promise<void> {
    try {
      const [status, instruction] = await Promise.all([
        call<ClientsStatusOutput>("clients_status"),
        call<InstructionOutput>("instruction"),
      ]);
      this.clients = status.clients;
      this.instruction = instruction;
    } catch {
      this.clients = [];
    }
  }

  async loadAutostart(): Promise<void> {
    try {
      this.autostart = await call<AutostartOutput>("autostart_get");
    } catch {
      this.autostart = null;
    }
  }

  /** Галочка «Запускать при входе в систему»: применяется сразу, без «Сохранить». */
  async setAutostart(enabled: boolean): Promise<void> {
    this.busy = "autostart";
    try {
      this.autostart = await call<AutostartOutput>("autostart_set", { enabled });
      app.flash(enabled ? "bars будет запускаться при входе в систему" : "Автозапуск выключен");
    } catch (error) {
      app.flash(error instanceof Error ? error.message : String(error), true);
      await this.loadAutostart();
    } finally {
      this.busy = null;
    }
  }

  async loadSecrets(): Promise<void> {
    const names = [this.saved?.embed.api_key_name, this.saved?.rerank.api_key_name].filter(
      (name): name is string => !!name,
    );
    for (const name of names) {
      try {
        this.secrets[name] = (await call<{ present: boolean }>("secret_status", { name })).present;
      } catch {
        this.secrets[name] = false;
      }
    }
  }

  reset(): void {
    if (this.saved) this.draft = clone(this.saved);
  }

  /** Число клиентов, которым нужно действие пользователя: метка на вкладке «Клиенты». */
  get clientsNeedAction(): number {
    return this.clients.filter((c) => c.stale || c.needs_hook_approval || !c.installed).length;
  }

  /** Сохраняет; при смене модели ждёт, пока воркер переведёт проекты в «нужна переиндексация», и открывает диалог согласия. */
  async save(): Promise<boolean> {
    if (!this.draft || !this.dirty || Object.keys(this.errors).length) return false;
    this.saving = true;
    const reindex = this.willReindex;
    try {
      await call("settings_save", { settings: this.draft });
      this.saved = clone(this.draft);
      await this.loadSecrets();
      app.flash("Настройки сохранены");
      if (reindex) await this.awaitConsent();
      return true;
    } catch (error) {
      app.flash(error instanceof Error ? error.message : String(error), true);
      return false;
    } finally {
      this.saving = false;
    }
  }

  /** Воркер проверяет проекты в очереди и не запускает эмбеддинг; ждём, пока он выставит `reindex_required`. */
  async awaitConsent(): Promise<void> {
    for (let attempt = 0; attempt < 30; attempt += 1) {
      await app.loadProjects();
      if (app.needReindex.length) {
        app.consentOpen = true;
        return;
      }
      const idle = !(app.activity?.running) && (app.activity?.queue.length ?? 0) === 0;
      if (attempt > 3 && idle) return;
      await wait(500);
      await app.refresh(false);
    }
  }

  /** Модель индекса возвращается в настройки: отпечатки снова совпадают, переиндексация не нужна (OQ-56). */
  async revertModel(indexFingerprint: string | null): Promise<void> {
    const parsed = parseFingerprint(indexFingerprint);
    if (!parsed || !this.saved) return;
    const next = clone(this.saved);
    next.embed.source = parsed.source;
    next.embed.model = parsed.model;
    try {
      await call("settings_save", { settings: next });
      this.saved = clone(next);
      this.draft = clone(next);
      app.consentOpen = false;
      await app.recheck();
      app.flash("Прежняя модель возвращена: переиндексация не нужна");
    } catch (error) {
      app.flash(error instanceof Error ? error.message : String(error), true);
    }
  }

  async download(id: string): Promise<void> {
    try {
      await call("model_download", { id });
      await this.loadModels();
      // Ход скачивания виден по `downloading`; опрашиваем, пока модель не станет скачанной.
      for (let attempt = 0; attempt < 600; attempt += 1) {
        await wait(1500);
        await this.loadModels();
        const all = [...(this.models?.embed ?? []), ...(this.models?.rerank ?? [])];
        if (!all.find(model => model.id === id)?.downloading) break;
      }
      const model = [...(this.models?.embed ?? []), ...(this.models?.rerank ?? [])].find((m) => m.id === id);
      if (model?.downloading) return;
      const failed = !model?.downloaded || !!model.download_error;
      app.flash(failed ? model?.download_error || `Не удалось скачать ${id}. Проверьте подключение и повторите.` : `Модель ${id} скачана`, failed);
    } catch (error) {
      app.flash(error instanceof Error ? error.message : String(error), true);
    }
  }

  async setSecret(name: string, value: string): Promise<boolean> {
    try {
      await call("secret_set", { name, value });
      this.secrets[name] = true;
      return true;
    } catch (error) {
      app.flash(error instanceof Error ? error.message : String(error), true);
      return false;
    }
  }

  async install(client: InstallClientName): Promise<void> {
    this.busy = client;
    try {
      const output = await call<{ changed_files: string[]; needs_hook_approval: boolean }>("install", { client });
      await this.loadClients();
      app.flash(
        output.changed_files.length
          ? `bars установлен: изменено файлов — ${output.changed_files.length}`
          : "bars уже установлен: файлы не менялись",
      );
    } catch (error) {
      app.flash(error instanceof Error ? error.message : String(error), true);
    } finally {
      this.busy = null;
    }
  }

  async uninstall(client: InstallClientName): Promise<void> {
    this.busy = client;
    try {
      await call("uninstall", { client });
      await this.loadClients();
      app.flash("bars удалён из клиента");
    } catch (error) {
      app.flash(error instanceof Error ? error.message : String(error), true);
    } finally {
      this.busy = null;
    }
  }
}

export const settings = new SettingsState();
