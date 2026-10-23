import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, tick, unmount } from "svelte";
import MissingLinkDialog from "./MissingLinkDialog.svelte";
import { missingLinkPath, followLink } from "./follow-link.svelte";
import { app } from "./app.svelte";
import { buildTree } from "./tree";
const api = vi.hoisted(() => ({ entryCreateDoc: vi.fn(), call: vi.fn(), inTauri: () => false }));
vi.mock("./api", () => ({...api, joinPath: (a: string,b: string) => `${a}/${b}`}));

describe("переход по ссылке в никуда", () => {
  beforeEach(() => { followLink.pending = null; followLink.error = ""; app.current = "project"; app.docsRoot = "docs"; app.tree = []; vi.clearAllMocks(); });
  it("имя рядом с источником; путь — от корня документации; alias и якорь не входят в имя", () => {
    expect(missingLinkPath("docs/a/x.md","docs","Новая тема#часть|текст",true)).toBe("docs/a/Новая тема.md");
    expect(missingLinkPath("docs/a/x.md","docs","b/Новая.md|текст",true)).toBe("docs/b/Новая.md");
    expect(missingLinkPath("x.md",".","Тема",true)).toBe("Тема.md");
    expect(missingLinkPath("docs/a/x.md","docs","../новая%20тема.md#x",false)).toBe("docs/новая тема.md");
  });
  it("выход за корень, абсолютные пути и служебные разделители не допускаются", () => {
    for (const raw of ["../вне", "/tmp/файл", "C:/файл", "a/../../вне", "a//b"]) expect(missingLinkPath("docs/a/x.md","docs",raw,true)).toBeNull();
  });
  it("переход только предлагает создание, запись — после нажатия", async () => {
    const open = vi.spyOn(app,"openDocument").mockResolvedValue(); const tree = vi.spyOn(app,"loadTree").mockResolvedValue();
    await followLink.open("project","docs/a/x.md","Новая",true);
    expect(api.entryCreateDoc).not.toHaveBeenCalled();
    expect(followLink.pending?.path).toBe("docs/a/Новая.md");
    api.entryCreateDoc.mockResolvedValue({status:"ok",path:"docs/a/Новая.md"});
    await followLink.create();
    expect(api.entryCreateDoc).toHaveBeenCalledWith("project","docs/a","Новая.md");
    expect(open).toHaveBeenCalledWith(expect.objectContaining({path:"docs/a/Новая.md"}));
    open.mockRestore(); tree.mockRestore();
  });
  it("ссылки на Index.md открываются; занятое имя остаётся ошибкой без перезаписи", async () => {
    app.tree = buildTree([{path:"docs/a/Index.md",title:"a",module:"a",chunks:1}],"docs");
    const open = vi.spyOn(app,"openDocument").mockResolvedValue();
    await followLink.open("project","docs/x.md","a/Index",true);
    expect(open).toHaveBeenCalled(); expect(followLink.pending).toBeNull();
    await followLink.open("project","docs/x.md","Нет",true);
    api.entryCreateDoc.mockResolvedValue({status:"error",message:"Уже есть",kind:"exists"});
    await followLink.create();
    expect(followLink.error).toBe("Уже есть"); expect(followLink.pending).not.toBeNull(); open.mockRestore();
  });

  it("диалог показывает рассчитанный путь, создаёт по кнопке и оставляет отказ видимым", async () => {
    const show = Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype,"showModal");
    Object.defineProperty(HTMLDialogElement.prototype,"showModal", {configurable:true,value:function(this: HTMLDialogElement) { this.setAttribute("open", ""); }});
    await followLink.open("project","docs/a/x.md","Новая",true);
    const target = document.createElement("div"); document.body.append(target);
    const component = mount(MissingLinkDialog,{target});
    try {
      flushSync(); await tick();
      expect(target.querySelector("dialog")?.open).toBe(true);
      expect(target.textContent).toContain("docs/a/Новая.md");
      expect(api.entryCreateDoc).not.toHaveBeenCalled();
      api.entryCreateDoc.mockResolvedValue({status:"error",kind:"exists",message:"Уже есть"});
      target.querySelector<HTMLButtonElement>("button.btn")!.click();
      await tick(); flushSync();
      expect(api.entryCreateDoc).toHaveBeenCalledTimes(1);
      expect(target.querySelector('[role="alert"]')?.textContent).toBe("Уже есть");
    } finally { await unmount(component); target.remove(); if (show) Object.defineProperty(HTMLDialogElement.prototype,"showModal",show); else Reflect.deleteProperty(HTMLDialogElement.prototype,"showModal"); }
  });
});
