import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';
import { test } from 'node:test';

test('каждый MCP-инструмент имеет ровно одно действие пользователя в таблице Т-37', () => {
  const registry = readFileSync(new URL('../crates/bars-mcp/src/lib.rs', import.meta.url), 'utf8');
  const document = readFileSync(new URL('../docs/features/agent-tools/window-parity.md', import.meta.url), 'utf8');
  const tools = [...registry.matchAll(/name = "(bars_\w+)"/g)].map(match => match[1]).sort();
  const rows = [...document.matchAll(/^\| `(bars_\w+)` \| ([^|]+) \| ([^|]+) \|$/gm)];
  assert.ok(tools.length > 0);
  assert.deepEqual(rows.map(row => row[1]).sort(), tools);
  assert.ok(rows.every(row => row[2].trim().length > 0 && row[3].trim().length > 0));
});
