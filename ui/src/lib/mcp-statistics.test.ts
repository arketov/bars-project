import { describe, it, expect } from "vitest";
import { selectStatistics, totals, type McpStatistic } from "./mcp-statistics";
const row = (session: string, project: string, calls: number, total_ms: number): McpStatistic => ({session, project, calls, total_ms, tool:"bars_read",errors:1,first:1,last:2});
describe("Статистика MCP", () => {
  const rows = [row("a","p",3,30),row("a","q",1,90),row("b","p",1,20),row("b","",1,10)];
  it("совместные фильтры и отдельная группа без проекта", () => {
    expect(selectStatistics(rows,"p","a")).toEqual([rows[0]]);
    expect(selectStatistics(rows,"*","a")).toHaveLength(2);
    expect(selectStatistics(rows,"","")).toEqual([rows[3]]);
  });
  it("взвешенное среднее и пустая выборка", () => {
    expect(totals(rows).average).toBe(25);
    expect(totals([])).toEqual({calls:0,errors:0,total_ms:0,average:0});
  });
});
