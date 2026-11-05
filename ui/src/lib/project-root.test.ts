import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import ProjectRootDialog from "./ProjectRootDialog.svelte";
import { app } from "./app.svelte";
import { call, pickFolder, projectRootPrepare, projectRootSuggest } from "./api";

vi.mock("./api", () => ({
  call: vi.fn(), joinPath: vi.fn(), openPath: vi.fn(), pickFolder: vi.fn(), inTauri: () => false,
  projectRootSuggest: vi.fn(), projectRootPrepare: vi.fn(),
}));
const invoke = vi.mocked(call);
const mounted: ReturnType<typeof mount>[] = [];
const settle = async () => { await new Promise(resolve => setTimeout(resolve, 0)); flushSync(); };
function show() {
  const host = document.createElement("div"); document.body.append(host);
  mounted.push(mount(ProjectRootDialog, { target: host })); flushSync(); return host;
}
beforeEach(() => {
  HTMLDialogElement.prototype.showModal = function() { this.setAttribute("open", ""); };
  app.projects = []; app.current = null; app.rootChoice = null; app.rootBusy = false;
  invoke.mockImplementation(async method => {
    if (method === "projects") return { projects: [] };
    if (method === "register" || method === "unregister") return {};
    throw new Error(`unexpected ${method}`);
  });
});
afterEach(async () => {
  for (const component of mounted.splice(0)) await unmount(component);
  app.rootChoice = null; app.stop(); document.body.replaceChildren(); vi.clearAllMocks();
});

it("suggests docs/ when the folder has it, otherwise the whole folder", async () => {
  vi.mocked(pickFolder).mockResolvedValue("C:/заметки");
  vi.mocked(projectRootSuggest).mockResolvedValue({ has_docs: false, suggested: "all" });
  await app.addProject();
  expect(app.rootChoice).toMatchObject({ path: "C:/заметки", suggested: "all", hasDocs: false });
  let host = show();
  expect((host.querySelector('input[value="all"]') as HTMLInputElement).checked).toBe(true);
  expect(host.textContent).toContain("docs/ пока нет");
  await unmount(mounted.pop()!); document.body.replaceChildren();

  vi.mocked(pickFolder).mockResolvedValue("C:/код");
  vi.mocked(projectRootSuggest).mockResolvedValue({ has_docs: true, suggested: "docs" });
  await app.addProject();
  host = show();
  expect((host.querySelector('input[value="docs"]') as HTMLInputElement).checked).toBe(true);
});

it("cancel registers nothing; confirm writes the choice before registering", async () => {
  vi.mocked(pickFolder).mockResolvedValue("C:/заметки");
  vi.mocked(projectRootSuggest).mockResolvedValue({ has_docs: false, suggested: "all" });
  await app.addProject();
  app.cancelRoot();
  expect(app.rootChoice).toBeNull(); expect(invoke).not.toHaveBeenCalled(); expect(projectRootPrepare).not.toHaveBeenCalled();

  await app.addProject();
  const order: string[] = [];
  vi.mocked(projectRootPrepare).mockImplementation(async () => { order.push("prepare"); });
  invoke.mockImplementation(async method => { order.push(method); return method === "projects" ? { projects: [] } : {}; });
  await app.confirmRoot("docs");
  await settle();
  expect(projectRootPrepare).toHaveBeenCalledWith("C:/заметки", "docs");
  expect(order.slice(0, 2)).toEqual(["prepare", "register"]);
  expect(app.rootChoice).toBeNull();
});
