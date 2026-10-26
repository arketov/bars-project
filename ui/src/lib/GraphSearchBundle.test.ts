import { expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import GraphSearchBundle from "./GraphSearchBundle.svelte";
import type { GraphReply } from "./graph";
import { app } from "./app.svelte";

it("в инспекторе доступны все пакеты, отрицание, условие и цитата; удалённый источник закрыт", async () => {
  const reference = { id: "doc", kind: "document", revision: "saved" };
  const edge = { id: "edge", from: "a", to: "b", predicate: "depends_on", polarity: "negative", qualifiers: { условие: "без драйвера" }, origin: "explicit", evidence: ["proof"] };
  const graph = {
    items: Array.from({length: 5}, () => ({type:"relation",assertion:edge})),
    assertions: [edge], nodes: [{reference:{id:"a"},label:"А"},{reference:{id:"b"},label:"Б"}],
    sources: [{reference,path:"docs/a.md",docs_path:"a.md",title:"А"}],
    evidence: [{id:"proof",document:reference,span:{first_line:2,last_line:3},quote:"Сохранённое основание"}],
    used_bytes: 1000, truncated:false,limit_reasons:[],
  } as unknown as GraphReply;
  const target = document.createElement("div"); document.body.append(target);
  const choose = vi.fn();
  const component = mount(GraphSearchBundle,{target,props:{graph,embedded:true,onselect:choose,states:{"docs/a.md":"deleted_since_search"}}});
  try {
    expect(target.querySelectorAll("article")).toHaveLength(5);
    expect(target.textContent).toContain("НЕ depends_on");
    expect(target.textContent).toContain("без драйвера");
    expect(target.textContent).toContain("Сохранённое основание");
    const buttons = [...target.querySelectorAll<HTMLButtonElement>("button")];
    buttons.filter(b=>b.textContent?.includes("Выделить путь"))[4].click();
    expect(choose).toHaveBeenCalledWith(4);
    expect(buttons.filter(b=>b.textContent?.includes("Открыть текущий файл")).every(b=>b.disabled)).toBe(true);
  } finally { await unmount(component); target.remove(); }
});

it("изменённое основание открывает текущий файл без старого диапазона", async () => {
  const reference = {id:"doc",kind:"document",revision:"old"};
  const edge = {id:"e",from:"doc",to:"doc",predicate:"uses",polarity:"positive",qualifiers:{},origin:"explicit",evidence:["p"]};
  const graph = {items:[{type:"edge",edge}],sources:[{reference,path:"docs/a.md",title:"А"}],nodes:[],assertions:[],evidence:[{id:"p",document:reference,span:{first_line:90,last_line:100},quote:"Старая цитата"}],used_bytes:0,truncated:false} as unknown as GraphReply;
  const open = vi.spyOn(app,"openDocument").mockResolvedValue(undefined);
  const target = document.createElement("div"); document.body.append(target);
  const component=mount(GraphSearchBundle,{target,props:{graph,embedded:true,states:{"docs/a.md":"changed_since_search"}}});
  try {
    [...target.querySelectorAll<HTMLButtonElement>("button")].find(b=>b.textContent?.includes("Открыть текущий файл"))!.click();
    expect(open).toHaveBeenCalledWith(expect.objectContaining({path:"docs/a.md",first_line:0,last_line:0,expectedRevision:"old"}));
  } finally { await unmount(component); target.remove(); open.mockRestore(); }
});
