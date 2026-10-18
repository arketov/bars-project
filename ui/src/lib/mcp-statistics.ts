export interface McpStatistic {
  session: string; project: string; tool: string;
  calls: number; errors: number; total_ms: number; first: number; last: number;
}
