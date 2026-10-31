import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import RenameProjectDialog from "./RenameProjectDialog.svelte";
import { app } from "./app.svelte";

const api = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock("./api", () => ({ ...api, inTauri: () => false }));
const first = { name: "repo", path: "C:/work/repo", docs_root: "docs", state: "ready" as const, modules: [] };
let component: ReturnType<typeof mount> | undefined;
const settle = async () => { await new Promise(resolve => setTimeout(resolve, 0)); flushSync(); };
beforeEach(() => {
  app.projects = [first]; app.current = first.path; app.query = "запрос"; app.selected = new Set(["features"]);
  app.renamingProject = null; app.renameBusy = false; app.renameError = null;
  api.call.mockReset(); api.call.mockResolvedValue({ saved: true });
});
afterEach(async () => {
  if (component) await unmount(component); component = undefined;
  document.body.innerHTML = ""; app.renamingProject = null; app.renameBusy = false; app.renameError = null;
});

it("сохраняет имя без смены проекта, поиска и области", async () => {
  app.requestRenameProject(first.path);
  await app.renameProject("  Мои знания  ");
  expect(api.call).toHaveBeenCalledExactlyOnceWith("project_rename", { project: first.path, name: "Мои знания" });
  expect(app.project?.name).toBe("Мои знания"); expect(app.current).toBe(first.path);
  expect(app.query).toBe("запрос"); expect([...app.selected]).toEqual(["features"]);
  expect(app.renamingProject).toBeNull();
});

it("ошибка сохраняет диалог и прежнее имя; повтор успешен", async () => {
  app.requestRenameProject(first.path); api.call.mockRejectedValueOnce(new Error("Нет доступа"));
  await app.renameProject("Новое имя");
  expect(app.renameError).toBe("Нет доступа"); expect(app.project?.name).toBe("repo");
  expect(app.renamingProject?.path).toBe(first.path);
  await app.renameProject("Новое имя"); expect(app.project?.name).toBe("Новое имя");
});

it("отмена и пустое имя ничего не записывают", async () => {
  app.requestRenameProject(first.path); await app.renameProject("  ");
  expect(api.call).not.toHaveBeenCalled(); expect(app.renameError).toBeTruthy();
  app.cancelRenameProject(); expect(app.renamingProject).toBeNull(); expect(app.project?.name).toBe("repo");
});

it("диалог выделяет прежнее имя, отправляет форму и показывает ошибку", async () => {
  app.requestRenameProject(first.path);
  const host = document.createElement("div"); document.body.append(host);
  component = mount(RenameProjectDialog, { target: host }); flushSync();
  const input = document.querySelector("input")!;
  expect(input.value).toBe("repo"); expect(document.activeElement).toBe(input);
  input.value = "Новое имя"; input.dispatchEvent(new Event("input", { bubbles: true }));
  api.call.mockRejectedValueOnce(new Error("Не удалось записать"));
  document.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  await settle(); expect(document.querySelector('[role="alert"]')?.textContent).toBe("Не удалось записать");
});
