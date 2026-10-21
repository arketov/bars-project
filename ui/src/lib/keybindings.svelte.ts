import { inTauri } from './api';
import { catalog, bindingFor, fromEvent, label, platform, validate, runtimeOverrides, type Binding, type Overrides, type Snapshot } from './keybindings';
const empty = (): Snapshot => ({ profile: { version: 1, revision: 0, platforms: {} }, error: null, recovery: null });
class Keybindings {
  snapshot = $state<Snapshot>(empty());
  error = $state('');
  available = $state(false);
  recording = $state(false);
  active = $state<Overrides>({});
  async load() {
    try {
      if (!inTauri()) throw new Error('Профиль сохраняется только в приложении bars');
      const { invoke } = await import('@tauri-apps/api/core');
      this.accept(await invoke<Snapshot>('keybindings_get'));
      this.available = true;
    } catch (error) { this.error = String(error); this.available = false; }
  }
  accept(snapshot: Snapshot) {
    if (!snapshot.error && snapshot.profile.revision < this.snapshot.profile.revision) return;
    if (snapshot.profile.version !== 1) { this.error = 'Неизвестная версия профиля'; return; }
    this.snapshot = snapshot;
    const overrides = snapshot.profile.platforms[platform] ?? {};
    const { active, errors } = runtimeOverrides(overrides, platform);
    this.active = snapshot.error ? {} : active;
    this.error = [snapshot.error, ...errors].filter(Boolean).join('; ');
  }
  async save(bindings: Overrides, revision: number, recovery: string | null) {
    const errors = validate(bindings, platform);
    if (errors.length) throw new Error(errors.join('; '));
    if (!this.available) throw new Error('Воркер недоступен; сохранение невозможно');
    const { invoke } = await import('@tauri-apps/api/core');
    const snapshot = await invoke<Snapshot>('keybindings_save', { platform, bindings, expectedRevision: revision, recovery });
    this.accept(snapshot);
  }
  hint(id: string) { const command = catalog.find(c => c.id === id); return command ? label(bindingFor(command, this.active, platform)) : ''; }
  aria(id: string) { const binding=this.binding(id); return binding ? [...binding.modifiers,binding.code.replace(/^Key|^Digit/,'').replace('Comma',',')].join('+') : undefined; }
  binding(id: string): Binding | null { const command = catalog.find(c => c.id === id); return command ? bindingFor(command, this.active, platform) : null; }
  match(event: KeyboardEvent) {
    const binding = fromEvent(event);
    if (!binding) return undefined;
    return catalog.find(c => !c.future && JSON.stringify(bindingFor(c, this.active, platform)) === JSON.stringify(binding));
  }
  async start() {
    if (!inTauri()) { await this.load(); return () => {}; }
    const { listen } = await import('@tauri-apps/api/event');
    const stop = await listen<Snapshot>('bars://keybindings', ({ payload }) => {
      if (payload.profile.revision > this.snapshot.profile.revision) this.accept(payload);
    });
    await this.load();
    return stop;
  }
}
export const keybindings = new Keybindings();
