import { describe, expect, it } from "vitest";
import { isRemote, rowTime, totals, totalsText, type ConsentRow } from "./consent";

const row = (path: string, chunks: number, settingsModel: string): ConsentRow => ({
  path,
  name: path,
  documents: 10,
  chunks,
  indexModel: "builtin:BAAI/bge-m3",
  settingsModel,
});

describe("consent", () => {
  it("итог считается только по отмеченным", () => {
    const rows = [row("a", 100, "builtin:BAAI/bge-m3"), row("b", 300, "builtin:BAAI/bge-m3")];
    const all = totals(rows, new Set(["a", "b"]));
    expect(all).toMatchObject({ projects: 2, documents: 20, chunks: 400, seconds: 200 });
    expect(totals(rows, new Set(["a"])).seconds).toBe(50);
    expect(totalsText(totals(rows, new Set()))).toBe("Ничего не выбрано");
  });

  it("без оценки время не выдумывается", () => {
    const rows = [row("a", 100, "remote:text-embedding-3-small")];
    const summary = totals(rows, new Set(["a"]));
    expect(summary.seconds).toBeNull();
    expect(summary.note).toBe("зависит от провайдера");
    expect(rowTime(rows[0])).toBe("зависит от провайдера");
    expect(totalsText(summary)).toContain("зависит от провайдера");
    expect(rowTime(row("x", 120, "builtin:BAAI/bge-m3"))).toBe("≈ 1 мин");
    expect(rowTime(row("y", 10, "builtin:Qwen/Qwen3-Embedding-0.6B"))).toBe("будет известно после первых пачек");
  });

  it("удалённая модель отличается от встроенной", () => {
    expect(isRemote("remote:x")).toBe(true);
    expect(isRemote("local-openai:x")).toBe(true);
    expect(isRemote("builtin:BAAI/bge-m3")).toBe(false);
  });
});
