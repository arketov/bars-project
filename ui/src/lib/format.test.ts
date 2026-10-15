import { describe, expect, it } from "vitest";
import {
  decimal,
  documents,
  duration,
  fit,
  humanName,
  moduleLabel,
  percentOf,
  plural,
  remainingSeconds,
  stateDot,
} from "./format";

describe("format", () => {
  it("имена для людей", () => {
    expect(humanName("hybrid-search.md")).toBe("Hybrid search");
    expect(humanName("agent-tools")).toBe("Agent tools");
    expect(humanName("(корень)")).toBe("(корень)");
    expect(moduleLabel(".")).toBe("(корень)");
  });

  it("оценка: цвет и метка по порогам", () => {
    expect(fit(0.87)).toEqual({ cls: "f-good", label: "хорошо подходит", percent: 87 });
    expect(fit(0.6).cls).toBe("f-mid");
    expect(fit(0.18).cls).toBe("f-low");
    expect(fit(1.4).percent).toBe(100);
  });

  it("склонения и числа", () => {
    expect(plural(1, ["документ", "документа", "документов"])).toBe("документ");
    expect(plural(3, ["документ", "документа", "документов"])).toBe("документа");
    expect(plural(11, ["документ", "документа", "документов"])).toBe("документов");
    expect(plural(21, ["документ", "документа", "документов"])).toBe("документ");
    expect(documents(214)).toBe("214 документов");
    expect(decimal(0.3)).toBe("0,30");
  });

  it("время и прогресс", () => {
    expect(duration(30)).toBe("меньше минуты");
    expect(duration(600)).toBe("10 мин");
    expect(duration(4800)).toBe("1 ч 20 мин");
    expect(duration(Number.NaN)).toBe("—");
    expect(remainingSeconds(0, 10, 5)).toBeNull();
    expect(remainingSeconds(10, 10, 5)).toBeNull();
    expect(remainingSeconds(25, 100, 50)).toBe(150);
    expect(percentOf(1, 0)).toBe(0);
    expect(percentOf(42, 100)).toBe(42);
  });

  it("точка проекта", () => {
    expect(stateDot("ready", 214, null, 0)).toEqual({ cls: "ok", text: "актуален · 214 документов" });
    expect(stateDot("indexing", 0, 42, 118).text).toBe("индексация · 42 % · ждут 118");
    expect(stateDot("indexing", 0, null, 0).text).toBe("индексация · ждёт");
    expect(stateDot("reindex_required", 0, null, 0).cls).toBe("warn");
    expect(stateDot("unavailable", 0, null, 0).cls).toBe("off");
  });
});
