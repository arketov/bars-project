// Настройки раскладки и порога близости: отдельно для мини-графа и полного графа (решение OQ-118), по проекту.
// Только удобство пользователя: хранятся в localStorage окна, при отказе хранилища действуют умолчания.
import { DEFAULT_FORCES, type Forces } from "./physics-core";
import { DEFAULT_MIN_STRENGTH, type Mode } from "./model";

export interface PanelSettings {
  forces: Forces;
  minS: number;
  mode: Mode;
}

export type PanelKind = "mini" | "full";

const MODES: Mode[] = ["links", "meaning", "chunks", "all"];

export const defaultSettings = (): PanelSettings => ({ forces: { ...DEFAULT_FORCES }, minS: DEFAULT_MIN_STRENGTH, mode: "links" });

const keyOf = (project: string, panel: PanelKind) => `bars.graph.${panel}.${project}`;

const finite = (value: unknown, min: number, max: number, fallback: number): number =>
  typeof value === "number" && Number.isFinite(value) ? Math.min(max, Math.max(min, value)) : fallback;

export function loadSettings(project: string, panel: PanelKind): PanelSettings {
  const base = defaultSettings();
  try {
    const raw = JSON.parse(localStorage.getItem(keyOf(project, panel)) ?? "null") as Partial<PanelSettings> | null;
    if (!raw || typeof raw !== "object") return base;
    const forces = (raw.forces ?? {}) as Partial<Forces>;
    return {
      forces: {
        link: finite(forces.link, 0, 4, base.forces.link),
        repel: finite(forces.repel, 0, 4, base.forces.repel),
        gravity: finite(forces.gravity, 0, 4, base.forces.gravity),
        length: finite(forces.length, 20, 160, base.forces.length),
      },
      minS: finite(raw.minS, 0.5, 0.95, base.minS),
      // Режим «по смыслу · чанки» требует раскрытых чанков, при открытии их нет.
      mode: MODES.includes(raw.mode as Mode) && raw.mode !== "chunks" ? (raw.mode as Mode) : base.mode,
    };
  } catch {
    return base;
  }
}

export function saveSettings(project: string, panel: PanelKind, settings: PanelSettings): void {
  try {
    localStorage.setItem(keyOf(project, panel), JSON.stringify(settings));
  } catch {
    // Хранилище недоступно (приватное окно, очищенные данные): настройки действуют до закрытия панели.
  }
}
