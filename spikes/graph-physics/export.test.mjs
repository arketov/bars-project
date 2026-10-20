import { test } from 'node:test';
import assert from 'node:assert/strict';
import { DatabaseSync } from 'node:sqlite';
import { mkdtempSync, mkdirSync, readFileSync, realpathSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { snapshot } from './export.mjs';
test('Снимок не меняет SQLite; проекция не теряет тип и различает слои', t => {
  const root = mkdtempSync(join(tmpdir(), 'bars-graph-physics-test-'));
  t.after(() => {
    const checked = realpathSync(root);
    assert(checked.startsWith(realpathSync(tmpdir())) && checked.includes('bars-graph-physics-test-'));
    rmSync(checked, { recursive: true });
  });
  mkdirSync(join(root, '.bars'));
  const file = join(root, '.bars/index.sqlite'), db = new DatabaseSync(file);
  db.exec('CREATE TABLE graph_nodes(id TEXT, data TEXT); CREATE TABLE graph_edges(id TEXT, data TEXT); CREATE TABLE graph_sources(path TEXT,revision TEXT); CREATE TABLE graph_meta(key TEXT,value TEXT)');
  const node = db.prepare('INSERT INTO graph_nodes VALUES (?,?)');
  for (const [id, kind, source_ids] of [['a','document',[]],['b','document',[]],['c','chunk',['a']],['s','section',['b']]]) node.run(id, JSON.stringify({ reference: { id, kind }, label: id, source_ids }));
  const edge = db.prepare('INSERT INTO graph_edges VALUES (?,?)');
  for (const [id, from, to, predicate, score] of [['e1','a','s','links_to',null],['e2','c','s','similar_to',0.8],['e3','a','c','contains',null],['e4','c','c','similar_to',1]]) edge.run(id, JSON.stringify({ id, from, to, predicate, score }));
  db.close();
  const hash = () => createHash('sha256').update(readFileSync(file)).digest('hex');
  const before = hash(), result = snapshot(root, 'test');
  assert.equal(hash(), before);
  assert.equal(result.views[0].nodes.length, 2);
  assert.deepEqual(result.views[0].edges.map(e => e.predicate).sort(), ['links_to','similar_to']);
  assert.equal(result.views[1].nodes.length, 3);
  for (const view of result.views) {
    const ids = new Set(view.nodes.map(n => n.id));
    assert(view.edges.every(e => ids.has(e.source) && ids.has(e.target) && e.source !== e.target));
  }
  assert.equal(result.views[0].edges.find(e => e.predicate === 'similar_to').weight, 0.8);
});
