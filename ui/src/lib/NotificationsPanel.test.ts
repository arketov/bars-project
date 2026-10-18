import { afterEach, expect, it } from "vitest";
import { flushSync, mount, tick, unmount } from "svelte";
import Notifications from "./Notifications.svelte";
import { app } from "./app.svelte";

const components: ReturnType<typeof mount>[] = [];
afterEach(async () => { for (const component of components.splice(0)) await unmount(component); document.body.innerHTML = ""; app.notifications = []; });

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
