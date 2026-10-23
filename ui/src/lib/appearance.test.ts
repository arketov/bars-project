import { describe, expect, it } from "vitest";
import { COLORS, ICONS, canStyleFolder, documentColor, emptyAppearance, folderColor, folderIcon, keyOf, validColor, validIcon, type AppearanceData } from "./appearance";

const data = (): AppearanceData => ({
  folders: {
    features: { color: "blue", icon: "book" },
    "features/search": { color: "violet", icon: "magnifier" },
    "features/search/deep": { color: "red", icon: "bug" },
    manual: { color: "mauve", icon: "ufo" },
  },
  documents: {
    "features/search/Index.md": { color: "pink", icon: null },
    "features/a.md": { color: null, icon: null },
  },
});

describe("Т-38: цвета и иконки", () => {
  it("наборы без повторов: девять цветов и двадцать иконок, как в Rust", () => {
    expect(new Set(COLORS).size).toBe(9);
    expect(new Set(ICONS).size).toBe(20);
  });

  it("неизвестное значение — отсутствие значения", () => {
    expect(validColor("mauve")).toBeNull();
    expect(validIcon("ufo")).toBeNull();
    expect(folderColor(data(), "manual")).toBeNull();
    expect(folderIcon(data(), "manual")).toBeNull();
  });

  it("выбирать можно у модуля и подпапки второго уровня, не у корня и не глубже", () => {
    expect(canStyleFolder("features")).toBe(true);
    expect(canStyleFolder("features/search")).toBe(true);
    expect(canStyleFolder("features/search/deep")).toBe(false);
    expect(canStyleFolder(".")).toBe(false);
    expect(canStyleFolder("")).toBe(false);
  });

  it("папка третьего уровня показывает цвет ближайшей папки выше и обычную иконку; запись на этом уровне игнорируется", () => {
    expect(folderColor(data(), "features/search/deep")).toBe("violet");
    expect(folderIcon(data(), "features/search/deep")).toBeNull();
    expect(folderIcon(data(), "features/search")).toBe("magnifier");
  });

  it("документ: свой цвет, иначе цвет ближайшей папки, иначе никакого", () => {
    expect(documentColor(data(), "features/search/Index.md")).toBe("pink");
    expect(documentColor(data(), "features/search/other.md")).toBe("violet");
    expect(documentColor(data(), "features/search/deep/x.md")).toBe("violet");
    expect(documentColor(data(), "features/a.md")).toBe("blue");
    expect(documentColor(data(), "manual/x.md")).toBeNull();
    expect(documentColor(data(), "root.md")).toBeNull();
    expect(documentColor(emptyAppearance(), "features/a.md")).toBeNull();
  });

  it("корневые файлы дерева приходят как ./имя — ключ без префикса", () => {
    expect(keyOf("./MAIN_IDEA.md")).toBe("MAIN_IDEA.md");
    expect(keyOf("features/a.md")).toBe("features/a.md");
  });
});
