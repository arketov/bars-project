import { beforeEach, describe, expect, it, vi } from "vitest";

const call = vi.hoisted(() => vi.fn());
vi.mock("./api", () => ({ call }));
import { linkTitle, loadLinks } from "./document-links";

describe("ссылки документа", () => {
  beforeEach(() => call.mockReset());

  it("один вызов links_of с точным проектом и путём", async () => {
    const reply = { known: true, outgoing: [], incoming: [{ path: "docs/b.md", lines: [3] }] };
    call.mockResolvedValueOnce(reply);
    expect(await loadLinks("/p", "docs/a.md")).toEqual(reply);
    expect(call).toHaveBeenCalledWith("links_of", { project: "/p", path: "docs/a.md" });
  });

  it("название: заголовок или имя файла", () => {
    expect(linkTitle("Бета", "docs/b.md")).toBe("Бета");
    expect(linkTitle(undefined, "docs/папка/Бета.md")).toBe("Бета");
  });
});
