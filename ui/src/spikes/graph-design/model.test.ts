import { describe, expect, it } from "vitest";
import {
  brightnessOf,
  drawStrength,
  springCoefficients,
  springOf,
  strengthOf,
  widthOf,
  type GEdge,
} from "./model";

const edge = (id: number, a: number, b: number, s: number, kind: GEdge["kind"] = "document_similarity"): GEdge => ({
  id,
  kind,
  a,
  b,
  raw: kind === "markdown_link" ? null : s,
  s,
  support: 1,
  lines: [],
});

describe("сила связи для рисунка", () => {
  it("ссылка — факт: s=1, близость ограничена [0,1]", () => {
    expect(strengthOf("markdown_link", null)).toBe(1);
    expect(strengthOf("document_similarity", -0.2)).toBe(0);
    expect(strengthOf("chunk_similarity", 1.4)).toBe(1);
    expect(strengthOf("document_similarity", 0.73)).toBe(0.73);
  });

  it("толщина, яркость и притяжение монотонно растут с силой", () => {
    for (let s = 0; s < 1; s += 0.02) {
      expect(widthOf(s + 0.02)).toBeGreaterThan(widthOf(s));
      expect(brightnessOf(s + 0.02)).toBeGreaterThan(brightnessOf(s));
      expect(springOf(s + 0.02)).toBeGreaterThan(springOf(s));
    }
    expect(widthOf(0.5)).toBeCloseTo(1.5);
    expect(brightnessOf(1)).toBeCloseTo(0.9);
  });

  it("шкала не зависит от остальных рёбер экрана", () => {
    const weak = edge(1, 0, 1, 0.6);
    const before = drawStrength(weak, "project");
    springCoefficients([weak, edge(2, 2, 3, 0.95)], (n) => n);
    expect(drawStrength(weak, "project")).toBe(before);
  });

  it("окно модели растягивает близость монотонно и не трогает ссылку", () => {
    const strong = edge(1, 0, 1, 0.9);
    const weak = edge(2, 0, 2, 0.65);
    expect(drawStrength(strong, "window")).toBeGreaterThan(drawStrength(weak, "window"));
    expect(drawStrength(edge(3, 0, 3, 1, "markdown_link"), "window")).toBe(1);
    expect(drawStrength(edge(4, 0, 4, 0.5), "window")).toBe(0);
  });
});

describe("суммарное притяжение пары владельцев", () => {
  it("не растёт от числа рёбер: сумма равна Q=max(q)", () => {
    for (const n of [1, 2, 50, 500]) {
      const edges = Array.from({ length: n }, (_, i) => edge(i, 1000 + i, 5000 + i, 0.6 + (i % 5) * 0.08, "chunk_similarity"));
      const owner = (node: number) => (node >= 5000 ? 2 : 1);
      const k = springCoefficients(edges, owner);
      const sum = [...k.values()].reduce((a, b) => a + b, 0);
      const q = Math.max(...edges.map((e) => springOf(e.s)));
      expect(sum).toBeCloseTo(q, 9);
    }
  });

  it("при одном ребре коэффициент равен q; сильнее ребро — больше доля", () => {
    const only = springCoefficients([edge(1, 0, 1, 0.8)], (n) => n);
    expect(only.get(1)).toBeCloseTo(springOf(0.8));
    const pair = springCoefficients([edge(1, 0, 1, 0.9), edge(2, 0, 1, 1, "markdown_link")], (n) => n);
    expect(pair.get(2)!).toBeGreaterThan(pair.get(1)!);
  });

  it("внутридокументные связи нормализуются отдельно от междокументных", () => {
    const owner = (node: number) => (node < 10 ? 1 : 2);
    const k = springCoefficients([edge(1, 0, 1, 0.8, "chunk_similarity"), edge(2, 0, 10, 0.8, "chunk_similarity")], owner);
    expect(k.get(1)).toBeCloseTo(springOf(0.8));
    expect(k.get(2)).toBeCloseTo(springOf(0.8));
  });
});
