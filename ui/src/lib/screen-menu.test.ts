// Шаг 3.3.7: меню остальных объектов — вкладка служебной панели, ссылка, заголовок структуры, строки экранов, находки.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import { app } from "./app.svelte";
import ContextMenu from "./ContextMenu.svelte";
import { commandAdapter } from "./command-adapters";
import { followLink } from "./follow-link.svelte";
import { headingMenu, linkMenu, panelTabMenu, rowMenu, topicMenu } from "./menu-model";
import { buildTree } from "./tree";
import { registerTabHost, type TabHost } from "./workspace-tabs";

const api = vi.hoisted(() => ({ call: vi.fn(), documentAction: vi.fn(), documentEditors: vi.fn(), copyText: vi.fn() }));
vi.mock("./api", () => ({ ...api, joinPath: (a: string, b: string) => `${a}/${b}`, openPath: vi.fn(), pickFolder: vi.fn(), inTauri: () => false }));

const components: ReturnType<typeof mount>[] = [];
const project = { path: "C:/проект", name: "Проект", docs_root: "docs", state: "ready" as const, modules: [] };
const settle = async () => { await new Promise(resolve => setTimeout(resolve, 0)); flushSync(); };
const cleanup: (() => void)[] = [];

function host() { const element = document.createElement("div"); document.body.append(element); return element; }
async function rightClick(element: Element) {
  element.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 100, clientY: 100 })); await settle();
}
const items = () => Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'));
const name = (item: Element) => item.querySelector("span")!.childNodes[0].textContent;
const labels = () => items().map(name);
const exact = (text: string) => items().find(item => name(item) === text)!;
const disabled = (text: string) => exact(text).getAttribute("aria-disabled") === "true";
const menu = () => { components.push(mount(ContextMenu, { target: host() })); flushSync(); };

beforeEach(() => {
  app.projects = [project]; app.current = project.path;
  app.tree = buildTree([{ path: "docs/a.md", module: ".", title: "А", chunks: 1 }, { path: "docs/b.md", module: ".", title: "Б", chunks: 1 }], "docs");
  vi.spyOn(app, "openDocument").mockResolvedValue(undefined as never);
  vi.spyOn(app, "openInNewWindow").mockResolvedValue(undefined as never);
});
afterEach(async () => {
  for (const component of components.splice(0)) await unmount(component);
  cleanup.splice(0).forEach(stop => stop());
  followLink.pending = null;
  app.current = null; app.projects = []; app.tree = [];
  document.body.innerHTML = ""; vi.restoreAllMocks(); vi.clearAllMocks();
});

describe("Модель меню остальных объектов", () => {
  it("вкладка служебной панели — только группа вкладки с причинами недоступности", () => {
    const rows = panelTabMenu({ others: 0, right: 0, canMove: false });
    expect(rows.map(row => row.label)).toEqual(["Закрыть", "Закрыть другие", "Закрыть справа", "Переместить вкладку…"]);
    expect(rows.map(row => !!row.disabled)).toEqual([false, true, true, true]);
  });

  it("ссылка с целью: открыть, новое окно, скопировать; битая: создать и скопировать, путь без допуска — серый", () => {
    expect(linkMenu({ resolved: true, canCreate: false }).map(row => row.label)).toEqual(["Открыть цель", "Открыть в новом окне", "Скопировать ссылку"]);
    const broken = linkMenu({ resolved: false, canCreate: false });
    expect(broken.map(row => row.label)).toEqual(["Создать документ", "Скопировать ссылку"]);
    expect(broken[0]).toMatchObject({ disabled: true, hint: "Недопустимый путь ссылки" });
    expect(linkMenu({ resolved: false, canCreate: true })[0].disabled).toBe(false);
  });

  it("строки экранов: пункта нет без обработчика, «Повторить» прячется, пока повтор недоступен", () => {
    expect(rowMenu("indexing", () => null)).toEqual([]);
    const ask = (retry: string | undefined) => rowMenu("notification", id => id === "notification.retry" ? retry : undefined);
    expect(ask("Повтор недоступен").map(row => row.label)).toEqual(["Подробности"]);
    expect(ask(undefined).map(row => row.label)).toEqual(["Подробности", "Повторить"]);
    expect(rowMenu("log", () => undefined).map(row => row.label)).toEqual(["Скопировать журнал"]);
    expect(rowMenu("mcp", () => "Нет данных")[0]).toMatchObject({ disabled: true, hint: "Нет данных" });
  });

  it("заголовок структуры и строка тем: пункты по обработчикам, закрытие темы опасное и серое при занятости", () => {
    expect(headingMenu(() => undefined).map(row => row.label)).toEqual(["Перейти к заголовку", "Скопировать ссылку на заголовок"]);
    expect(headingMenu(id => id === "heading.copyLink" ? "Документа нет в дереве проекта" : undefined).at(-1)).toMatchObject({ disabled: true });
    const topic = topicMenu(true);
    expect(topic.map(row => row.label)).toEqual(["Скопировать запрос", "Закрыть тему"]);
    expect(topic.at(-1)).toMatchObject({ danger: true, disabled: true, hint: "Идёт операция с темами" });
  });
});

describe("Вкладка служебной панели", () => {
  it("правый клик открывает группу вкладки, команда идёт в хозяина вкладок с нажатой панелью", async () => {
    const run = vi.fn(async () => {});
    const stub: TabHost = { panelAt: element => element.closest<HTMLElement>("[data-workspace-panel]")?.dataset.workspacePanel, state: () => ({ others: 1, right: 0, canMove: true }), run };
    cleanup.push(registerTabHost(stub));
    const tab = host(); tab.dataset.workspacePanel = "logs"; tab.dataset.menuKind = "panel-tab"; tab.dataset.menuPlace = "tab";
    menu(); await rightClick(tab);
    expect(labels()).toEqual(["Закрыть", "Закрыть другие", "Закрыть справа", "Переместить вкладку…"]);
    expect(disabled("Закрыть справа")).toBe(true);
    exact("Закрыть другие").click(); await settle();
    expect(run).toHaveBeenCalledWith("tab.closeOthers", "logs");
  });

  it("без хозяина вкладок (отдельное окно) меню нет", async () => {
    const tab = host(); tab.dataset.workspacePanel = "logs"; tab.dataset.menuKind = "panel-tab";
    menu(); await rightClick(tab);
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });
});

describe("Ссылка в тексте и в панели «Ссылки»", () => {
  function page(inner: string) {
    const section = host(); section.dataset.documentPath = "docs/a.md"; section.dataset.menuKind = "file"; section.dataset.menuPlace = "render";
    section.innerHTML = inner; return section;
  }

  it("ссылка с документом-целью: открыть цель, в новом окне, скопировать как [[Название]]", async () => {
    const section = page('<a data-menu-kind="link" data-wiki-link="b#Раздел|подпись">подпись</a>');
    menu(); await rightClick(section.querySelector("a")!);
    expect(labels()).toEqual(["Открыть цель", "Открыть в новом окне", "Скопировать ссылку"]);
    exact("Открыть цель").click(); await settle();
    expect(app.openDocument).toHaveBeenCalledWith(expect.objectContaining({ path: "docs/b.md" }));
    await rightClick(section.querySelector("a")!);
    exact("Открыть в новом окне").click(); await settle();
    expect(app.openInNewWindow).toHaveBeenCalledWith("docs/b.md");
    await rightClick(section.querySelector("a")!);
    exact("Скопировать ссылку").click(); await settle();
    expect(api.copyText).toHaveBeenCalledWith("[[b#Раздел]]");
  });

  it("битая ссылка: создать документ запускает прежний диалог, копирование даёт ссылку как написана", async () => {
    const section = page('<a data-menu-kind="link" data-wiki-link="Нет такого">Нет такого</a><a data-menu-kind="link" data-doc-link="x.md">текст</a>');
    menu(); await rightClick(section.querySelectorAll("a")[0]);
    expect(labels()).toEqual(["Создать документ", "Скопировать ссылку"]);
    exact("Создать документ").click(); await settle();
    expect(followLink.pending).toEqual({ project: project.path, path: "docs/Нет такого.md" });
    await rightClick(section.querySelectorAll("a")[1]);
    exact("Скопировать ссылку").click(); await settle();
    expect(api.copyText).toHaveBeenCalledWith("[текст](x.md)");
  });

  it("исходящая строка панели «Ссылки» работает как ссылка; без документа-источника меню нет", async () => {
    const section = page('<ul><li data-menu-kind="link" data-doc-link="b.md" data-link-text="Б"><button>Б</button></li></ul>');
    menu(); await rightClick(section.querySelector("button")!);
    expect(labels()).toEqual(["Открыть цель", "Открыть в новом окне", "Скопировать ссылку"]);
    const alone = host(); alone.innerHTML = '<a data-menu-kind="link" data-wiki-link="b">Б</a>';
    await rightClick(alone.querySelector("a")!);
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });

  it("цель из индекса ссылок важнее разбора текста: строка с известной целью не считается битой", async () => {
    const section = page('<ul><li data-menu-kind="link" data-doc-link="не/так/написано.md" data-link-target="docs/b.md"><button>Б</button></li></ul>');
    menu(); await rightClick(section.querySelector("button")!);
    expect(labels()).toEqual(["Открыть цель", "Открыть в новом окне", "Скопировать ссылку"]);
    exact("Открыть цель").click(); await settle();
    expect(app.openDocument).toHaveBeenCalledWith(expect.objectContaining({ path: "docs/b.md" }));
  });

  it("смена проекта закрывает меню ссылки; устаревшая цель не выполняется", async () => {
    const section = page('<a data-menu-kind="link" data-wiki-link="b">Б</a>');
    menu(); await rightClick(section.querySelector("a")!);
    app.current = "другой"; await settle();
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });
});

describe("Заголовок структуры", () => {
  it("переход и ссылка на заголовок вызывают обработчики строки, а не соседей", async () => {
    const section = host(); section.dataset.documentPath = "docs/a.md"; section.dataset.menuKind = "file"; section.dataset.menuPlace = "render";
    const first = document.createElement("button"), second = document.createElement("button");
    first.dataset.menuKind = second.dataset.menuKind = "heading"; section.append(first, second);
    const goTo = vi.fn(), goOther = vi.fn();
    for (const [element, run] of [[first, goTo], [second, goOther]] as const) {
      const action = commandAdapter(element, { "heading.goTo": { run }, "heading.copyLink": { run: async () => { await api.copyText(`[[a#${element === first ? "Один" : "Два"}]]`); } } });
      cleanup.push(() => action.destroy());
    }
    menu(); await rightClick(second);
    expect(labels()).toEqual(["Перейти к заголовку", "Скопировать ссылку на заголовок"]);
    exact("Скопировать ссылку на заголовок").click(); await settle();
    expect(api.copyText).toHaveBeenCalledWith("[[a#Два]]");
    await rightClick(second); exact("Перейти к заголовку").click(); await settle();
    expect(goOther).toHaveBeenCalledOnce(); expect(goTo).not.toHaveBeenCalled();
  });
});

describe("Строки индексации, MCP, уведомлений и журнала", () => {
  function row(kind: string, commands: Record<string, { run: () => void; reason?: () => string | undefined }>) {
    const element = host(); element.dataset.menuKind = "row"; element.dataset.menuRow = kind; element.textContent = "строка";
    const action = commandAdapter(element, commands); cleanup.push(() => action.destroy());
    return element;
  }

  it("уведомление: «Повторить» есть, когда повтор допустим, и вызывает обработчик строки", async () => {
    const retry = vi.fn(), details = vi.fn();
    const element = row("notification", { "notification.details": { run: details }, "notification.retry": { run: retry } });
    menu(); await rightClick(element);
    expect(labels()).toEqual(["Подробности", "Повторить"]);
    exact("Повторить").click(); await settle();
    expect(retry).toHaveBeenCalledOnce(); expect(details).not.toHaveBeenCalled();
  });

  it("уведомление без допустимого повтора: только «Подробности»; журнал: «Скопировать журнал»", async () => {
    const element = row("notification", { "notification.details": { run: vi.fn() }, "notification.retry": { run: vi.fn(), reason: () => "Повтор недоступен" } });
    menu(); await rightClick(element);
    expect(labels()).toEqual(["Подробности"]);
    const log = row("log", { "logs.copy": { run: vi.fn() } });
    await rightClick(log);
    expect(labels()).toEqual(["Скопировать журнал"]);
  });

  it("серая команда показывает причину и не выполняется; строка без обработчиков меню не получает", async () => {
    const run = vi.fn();
    const element = row("mcp", { "mcp.details": { run, reason: () => "Нет данных" } });
    menu(); await rightClick(element);
    expect(disabled("Подробности")).toBe(true);
    exact("Подробности").click(); await settle();
    expect(run).not.toHaveBeenCalled();
    const bare = host(); bare.dataset.menuKind = "row"; bare.dataset.menuRow = "indexing";
    await rightClick(bare);
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });

  it("ошибка обработчика показывается уведомлением, а не успехом", async () => {
    const element = row("log", { "logs.copy": { run: () => { throw new Error("Буфер недоступен"); } } });
    menu(); await rightClick(element);
    exact("Скопировать журнал").click(); await settle();
    expect(app.toast).toEqual({ text: "Буфер недоступен", bad: true });
  });
});

describe("Находки, вхождения, похожие отрывки и входящие ссылки", () => {
  it("объект со строкой получает меню документа на месте карточки и открывает документ на этой строке", async () => {
    const section = host(); section.dataset.documentPath = "docs/a.md"; section.dataset.menuKind = "file"; section.dataset.menuPlace = "tab";
    section.innerHTML = '<li data-document-path="docs/b.md" data-document-line="7" data-menu-kind="file" data-menu-place="card"><button>Б</button></li>';
    menu(); await rightClick(section.querySelector("button")!);
    expect(labels().slice(0, 3)).toEqual(["Открыть", "Открыть в новом окне", "Открыть в …"]);
    expect(labels()).not.toContain("Закрыть");
    exact("Открыть").click(); await settle();
    expect(app.openDocument).toHaveBeenCalledWith(expect.objectContaining({ path: "docs/b.md", first_line: 7, last_line: 7 }));
  });
});
