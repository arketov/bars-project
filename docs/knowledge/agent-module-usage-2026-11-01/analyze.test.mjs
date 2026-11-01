import { test } from 'node:test';
import assert from 'node:assert/strict';
import { scopeOf, summarize, report } from './analyze.mjs';

const search = (id, session, scope, project = 'bars-project', proxyAttempt = null) => ({ id, started: id, session, scope, project, proxyAttempt, tool: 'bars_rag_search', outcome: 'success' });
const projects = (id, session) => ({ id, started: id, session, project: 'none', tool: 'bars_projects', outcome: 'success', proxyAttempt: null });

test('missing, null, empty, both and incomplete input have distinct treatment', () => {
  assert.equal(scopeOf({}), 'broad');
  assert.equal(scopeOf({ modules: [], files: null }), 'broad');
  assert.equal(scopeOf({ modules: ['features'], files: ['a.md'] }), 'both');
  assert.equal(scopeOf({ modules: 'features' }), 'unknown');
  assert.equal(scopeOf({ modules: ['features'] }, ['original_mcp_input_unavailable']), 'unknown');
  assert.equal(scopeOf(null), 'unknown');
  assert.equal(scopeOf({ modules: ['features'] }, ['requested']), 'unknown');
  assert.equal(scopeOf({ modules: ['features'] }, ['requested.modules:items=101,saved=100']), 'unknown');
  assert.equal(scopeOf({ modules: ['features'] }, ['requested.query:bytes=2000,saved=100']), 'modules');
});

test('projects joins by session, not empty project; sessions overlap across projects', () => {
  const rows = [search(3, 's1', 'modules'), projects(1, 's1'), search(2, 's1', 'broad', 'overdrive-bots'), search(4, 's2', 'files'), projects(5, 's3')];
  const result = report(rows);
  assert.equal(result.attempts.sessionsWithProjects.numerator, 1);
  assert.equal(result.attempts.sessionsWithProjects.denominator, 2);
  assert.equal(result.attempts.firstObservedWithModules.numerator, 0);
  assert.equal(result.byProject['bars-project'].sessionsWithProjects.percent, 50);
  assert.equal(result.byProject['overdrive-bots'].sessionsWithProjects.percent, 100);
  const unresolved = report([search(1, 'x', 'broad', 'none'), projects(2, 'x')]);
  assert.equal(unresolved.byProject.none.searches, 1);
  assert.equal(unresolved.byProject.none.sessionsWithProjects.percent, 100);
});

test('errors count; only explicitly marked retries excluded, agent repeats retained', () => {
  const rows = [search(1, 's', 'both', 'bars-project', 1), { ...search(2, 's', 'both', 'bars-project', 2), outcome: 'error' }, search(3, 's', 'broad', 'bars-project', 1), search(4, 't', 'unknown')];
  const result = report(rows);
  assert.equal(result.attempts.searches, 4);
  assert.equal(result.attempts.outcomes.error, 1);
  assert.equal(result.attempts.withModules.percent, 50);
  assert.equal(result.attempts.withModulesKnown.denominator, 3);
  assert.equal(result.withoutMarkedProxyRetries.searches, 3);
  assert.equal(result.removedProxyRetries, 1);
  assert.equal(result.attempts.sessionsWithUnknownScope, 1);
});

test('zero denominator stays unavailable instead of zero percent', () => {
  assert.equal(summarize([]).sessionsWithProjects.percent, null);
  assert.equal(summarize([projects(1, 's')]).withModules.percent, null);
});
