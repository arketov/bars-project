import { afterEach, expect, it, vi } from "vitest";
import { flushSync, mount, tick, unmount } from "svelte";
import Notifications from "./Notifications.svelte";
import { app } from "./app.svelte";

const components: ReturnType<typeof mount>[] = [];
afterEach(async () => { for (const component of components.splice(0)) await unmount(component); document.body.innerHTML = ""; app.notifications = []; vi.restoreAllMocks(); vi.unstubAllGlobals(); });

it("shows progress, closes on Escape and restores focus", async () => {
  app.notifications = [{ id: "test", title: "Загрузка · bge", text: "Скачивание", percent: 42, active: true, bad: false, unread: false }];
  const target = document.createElement("div"); document.body.append(target);
  components.push(mount(Notifications, { target })); flushSync();
  const button = target.querySelector("button")!;
  button.click(); await tick(); flushSync();
  expect(target.querySelector("progress")?.value).toBe(42);
  expect(button.getAttribute("aria-expanded")).toBe("true");
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })); flushSync();
  expect(target.querySelector("section")).toBeNull();
  expect(document.activeElement).toBe(button);
});

it("shows empty state and closes on outside click", async () => {
  app.notifications = [];
  const target = document.createElement("div"); document.body.append(target);
  components.push(mount(Notifications, { target })); flushSync();
  target.querySelector("button")!.click(); await tick(); flushSync();
  expect(target.textContent).toContain("Уведомлений пока нет");
  document.body.click(); flushSync();
  expect(target.querySelector("section")).toBeNull();
});

it("reads only intersecting cards and traps Tab within the panel", async () => {
  const callbacks: IntersectionObserverCallback[] = [];
  vi.stubGlobal("IntersectionObserver", class {
    constructor(callback: IntersectionObserverCallback) { callbacks.push(callback); }
    observe() {} disconnect() {}
  });
  const read = vi.spyOn(app, "readNotifications").mockImplementation(() => {});
  app.notifications = [{ id: "visible", title: "Проверка", text: "В работе", active: true, bad: false, unread: true, percent: null }, { id: "hidden", title: "Ошибка", text: "Сбой", active: false, bad: true, unread: true, percent: null }];
  const target = document.createElement("div"); document.body.append(target);
  components.push(mount(Notifications, { target })); flushSync();
  target.querySelector("button")!.click(); await tick(); flushSync();
  expect(read).not.toHaveBeenCalled();
  callbacks[1]([{ isIntersecting: false, intersectionRatio: 0 } as IntersectionObserverEntry], {} as IntersectionObserver);
  expect(read).not.toHaveBeenCalled();
  callbacks[0]([{ isIntersecting: true, intersectionRatio: 1 } as IntersectionObserverEntry], {} as IntersectionObserver);
  expect(read).toHaveBeenCalledWith("visible", undefined);
  const panel = target.querySelector("section")!;
  const buttons = panel.querySelectorAll<HTMLButtonElement>("button:not(:disabled)");
  buttons[buttons.length - 1].focus();
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", cancelable: true }));
  expect(document.activeElement).toBe(buttons[0]);
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", shiftKey: true, cancelable: true }));
  expect(document.activeElement).toBe(buttons[buttons.length - 1]);
});
