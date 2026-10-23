import { afterEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import TreeRow from "./TreeRow.svelte";
import { app } from "./app.svelte";
import { buildTree } from "./tree";

vi.mock("./api", () => ({ call: vi.fn(), joinPath: (a: string, b: string) => `${a}/${b}`, openPath: vi.fn(), pickFolder: vi.fn(), inTauri: () => false }));

describe("Клик по папке открывает её Index.md (Т-32)", () => {
  afterEach(() => { document.body.innerHTML = ""; vi.restoreAllMocks(); });
  it("клик по имени открывает Index.md и раскрывает папку; чекбокс не меняется", () => {
    app.tree = buildTree([
      { path: "docs/модуль/Index.md", module: "модуль", title: "Index", chunks: 1 },
      { path: "docs/модуль/a.md", module: "модуль", title: "a", chunks: 1 },
    ], "docs");
    const open = vi.spyOn(app, "openDocument").mockResolvedValue(undefined as never);
    const el = document.createElement("div"); document.body.append(el);
    const c = mount(TreeRow, { target: el, props: { node: app.tree[0] } }); flushSync();
    el.querySelector<HTMLElement>(".nm")!.click(); flushSync();
    expect(open).toHaveBeenCalledWith(expect.objectContaining({ path: "docs/модуль/Index.md" }), true);
    expect(el.querySelector(".kids")).not.toBeNull();
    expect(el.querySelector<HTMLInputElement>("input")!.checked).toBe(false);
    void unmount(c);
  });
});
