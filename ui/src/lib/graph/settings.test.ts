import { beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_FORCES } from "./physics-core";
import { defaultSettings, loadSettings, saveSettings } from "./settings";

describe("настройки раскладки панели графа", () => {
  beforeEach(() => localStorage.clear());

  it("мини-граф и полный граф хранят настройки отдельно, по проекту (OQ-118)", () => {
    const mini = { ...defaultSettings(), minS: 0.9, mode: "meaning" as const };
    saveSettings("C:/a", "mini", mini);
    expect(loadSettings("C:/a", "mini")).toEqual(mini);
    expect(loadSettings("C:/a", "full")).toEqual(defaultSettings());
    expect(loadSettings("C:/b", "mini")).toEqual(defaultSettings());
  });

  it("значения ограничиваются допустимыми, испорченный JSON даёт умолчания", () => {
    localStorage.setItem("bars.graph.v2.full.C:/a", JSON.stringify({ forces: { link: 99, repel: -5, gravity: "x", length: 1 }, minS: 0.1, mode: "all" }));
    const loaded = loadSettings("C:/a", "full");
    expect(loaded.forces).toEqual({ link: 4, repel: 0, gravity: DEFAULT_FORCES.gravity, length: 20 });
    expect(loaded.minS).toBe(0.5);
    expect(loaded.mode).toBe("all");
    localStorage.setItem("bars.graph.v2.full.C:/a", "{не json");
    expect(loadSettings("C:/a", "full")).toEqual(defaultSettings());
  });

  it("режим чанков не восстанавливается: при открытии чанки не раскрыты", () => {
    saveSettings("C:/a", "full", { ...defaultSettings(), mode: "chunks" });
    expect(loadSettings("C:/a", "full").mode).toBe("links");
  });

  it("недоступное хранилище не ломает панель", () => {
    const get = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("заблокировано");
    });
    const set = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("заблокировано");
    });
    expect(loadSettings("C:/a", "mini")).toEqual(defaultSettings());
    expect(() => saveSettings("C:/a", "mini", defaultSettings())).not.toThrow();
    get.mockRestore();
    set.mockRestore();
  });
});
