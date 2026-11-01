import { expect, it } from "vitest";
import { mount, unmount } from "svelte";
import { explanation, graphNote, label, labels, limitText, type LabelGroup } from "./labels";
import GraphSearchBundle from "./GraphSearchBundle.svelte";
import type { GraphReply } from "./graph";

it("каждое значение словаря имеет русскую подпись и исходное значение в подсказке", () => {
  for (const [group, entries] of Object.entries(labels)) {
    for (const id of Object.keys(entries)) {
      const result = label(group as LabelGroup, id);
      expect(result.text).toMatch(/[А-Яа-яЁё]/);
      expect(result.text).not.toMatch(/[A-Za-z_=]/);
      expect(result.hint).toContain(id);
    }
    for (const id of ["future_value=1", "toString", "__proto__"]) {
      const result = label(group as LabelGroup, id);
      expect(result.text).not.toContain(id);
      expect(result.hint).toContain(id);
    }
  }
});

it("известные фразы переводятся целиком, неизвестная диагностика не обрезается", () => {
  expect(explanation("fallback", "Связи отключены: include_related=false").text).toBe("Связанные источники выключены");
  expect(explanation("fallback", "Связанные источники не поместились").text).toBe("Связанные источники не поместились");
  const raw = "Будущая причина: include_related=false; future_code";
  expect(explanation("fallback", raw)).toEqual({ text: "Причина изменения режима не определена", hint: raw });
  expect(limitText(["nodes", "max_nodes", "max_bytes", "future_limit"])).toBe("достигнут предел числа узлов; достигнут предел размера ответа; причина ограничения не определена");
  expect(graphNote("чанков показано 5: достигнут предел 5").text).toBe("разделов показано 5: достигнут предел 5");
});

it("связанные источники: видимые подписи без кодов, диагностика и цитаты сохранены", async () => {
  const edge = { id: "e", from: "a", to: "b", predicate: "depends_on", polarity: "negative", qualifiers: {}, origin: "explicit", evidence: [] };
  const graph = {
    items: [{ type: "relation", assertion: edge }], assertions: [edge],
    nodes: [{ reference: { id: "a" }, label: "Архитектура" }, { reference: { id: "b" }, label: "Решение" }],
    sources: [{ path: "docs/a.md", reference: { id: "doc" } }], evidence: [], used_bytes: 4321,
    truncated: true, limit_reasons: ["max_bytes", "future_limit"],
  } as unknown as GraphReply;
  const target = document.createElement("div");
  const component = mount(GraphSearchBundle, { target, props: { graph, related: true } });
  try {
    expect(target.textContent).toContain("НЕ зависит от");
    expect(target.textContent).toContain("Найдено связанных источников: 1");
    expect(target.textContent).not.toMatch(/depends_on|max_bytes|future_limit|4321/);
    expect([...target.querySelectorAll("[title]")].map(el => el.getAttribute("title")).join(" ")).toMatch(/max_bytes.*future_limit/);
    expect(graph.items[0]).toMatchObject({ assertion: { predicate: "depends_on" } });
  } finally { await unmount(component); }
});
