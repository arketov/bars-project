import { afterEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import McpStatisticsPage from "./McpStatisticsPage.svelte";
import { mcpStatistics } from "./api";
vi.mock("./api", () => ({mcpStatistics:vi.fn()}));
const read=vi.mocked(mcpStatistics);
const mounted: ReturnType<typeof mount>[]=[];
const row=(session:string,project:string,calls:number)=>({session,project,calls,tool:"bars_read",errors:0,total_ms:120,first:1700000000,last:1700000010});
async function settle(){await new Promise(r=>setTimeout(r,0));flushSync();}
function show(){const host=document.createElement("div");document.body.append(host);mounted.push(mount(McpStatisticsPage,{target:host}));flushSync();return host;}
afterEach(()=>{mounted.splice(0).forEach(c=>void unmount(c));document.body.replaceChildren();vi.resetAllMocks();});
describe("Статистика MCP — экран",()=>{
  it("фильтрует проект и сессию, сбрасывает сессию при смене проекта",async()=>{
    read.mockResolvedValue([row("a","p",3),row("b","p",1),row("b","q",2)]);
    const host=show();await settle();
    expect(host.textContent).toContain("6 вызовов");
    const [project,session]=host.querySelectorAll("select");
    project.value="p";project.dispatchEvent(new Event("change",{bubbles:true}));flushSync();
    expect(host.textContent).toContain("4 вызовов");
    session.value="a";session.dispatchEvent(new Event("change",{bubbles:true}));flushSync();
    expect(host.textContent).toContain("3 вызовов");
    project.value="q";project.dispatchEvent(new Event("change",{bubbles:true}));flushSync();
    expect(session.value).toBe("");expect(host.textContent).toContain("2 вызовов");
  });
  it("пустое состояние, ошибка и успешный повтор",async()=>{
    read.mockRejectedValueOnce("Ошибка хранилища").mockResolvedValue([]);
    const host=show();await settle();
    expect(host.querySelector('[role="alert"]')?.textContent).toContain("Ошибка хранилища");
    host.querySelector("button")!.click();await settle();
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(host.textContent).toContain("Пока нет вызовов");
  });
});
