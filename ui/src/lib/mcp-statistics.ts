export interface McpStatistic {
  session: string; project: string; tool: string;
  calls: number; errors: number; total_ms: number; first: number; last: number;
}

export function selectStatistics(rows: McpStatistic[], project: string, session: string): McpStatistic[] {
  return rows.filter(r => (project === "*" || r.project === project) && (!session || r.session === session));
}

export function totals(rows: McpStatistic[]) {
  const sum = rows.reduce((a, r) => ({ calls: a.calls + r.calls, errors: a.errors + r.errors, total_ms: a.total_ms + r.total_ms }), { calls: 0, errors: 0, total_ms: 0 });
  return { ...sum, average: sum.calls ? Math.round(sum.total_ms / sum.calls) : 0 };
}
