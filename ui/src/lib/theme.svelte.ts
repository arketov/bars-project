// Тема окна: «Как в системе» (по умолчанию), «Светлая», «Тёмная» (settings-window.md, palette.md).
// Выбор хранится в localStorage окна: это настройка оформления, а не поиска, и воркеру она не нужна.

export type ThemePref = "system" | "light" | "dark";

const KEY = "bars-theme";

function read(): ThemePref {
  try {
    const value = localStorage.getItem(KEY);
    if (value === "light" || value === "dark") return value;
  } catch {
    // Хранилище недоступно — остаётся «как в системе».
  }
  return "system";
}

/** Какой режим применяется на самом деле. */
export function resolveMode(pref: ThemePref, systemDark: boolean): "light" | "dark" {
  return pref === "system" ? (systemDark ? "dark" : "light") : pref;
}

class Theme {
  pref = $state<ThemePref>(read());
  mode = $state<"light" | "dark">("light");
  #media: MediaQueryList | null = null;

  start(): void {
    this.#media = window.matchMedia("(prefers-color-scheme: dark)");
    this.#media.addEventListener("change", () => this.apply());
    this.apply();
  }

  set(pref: ThemePref): void {
    this.pref = pref;
    try {
      localStorage.setItem(KEY, pref);
    } catch {
      // Не сохранилось — тема действует до закрытия окна.
    }
    this.apply();
  }

  /** Быстрое переключение светлой и тёмной под деревом. */
  toggle(): void {
    this.set(this.mode === "dark" ? "light" : "dark");
  }

  apply(): void {
    this.mode = resolveMode(this.pref, this.#media?.matches ?? false);
    document.documentElement.dataset.mode = this.mode;
  }
}

export const theme = new Theme();
