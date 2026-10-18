import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { DatabaseSync } from 'node:sqlite';

const script = fileURLToPath(new URL('./graph-embedding-probe.mjs', import.meta.url));
const run = (...args) => spawnSync(process.execPath, [script, ...args], { encoding: 'utf8' });
const json = p => JSON.parse(fs.readFileSync(p, 'utf8'));
const write = (p, data) => fs.writeFileSync(p, JSON.stringify(data));

function fixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'bars-graph-probe-'));
  fs.mkdirSync(path.join(root, '.bars'));
  const db = new DatabaseSync(path.join(root, '.bars', 'index.sqlite'));
  db.exec(`CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT);
    INSERT INTO meta VALUES('schema_version','1'),('model_dim','2'),('docs_root','docs');
    CREATE TABLE documents(id INTEGER PRIMARY KEY,path TEXT,module TEXT,title TEXT,hash TEXT);
    CREATE TABLE chunks(document_id INTEGER,ordinal INTEGER,first_line INTEGER,last_line INTEGER,heading_chain TEXT,text TEXT,embed_hash TEXT);
    CREATE TABLE vectors(embed_hash TEXT PRIMARY KEY,vector BLOB);`);
  const vector = Buffer.alloc(8); vector.writeFloatLE(1, 0);
  for (const [i, name] of ['features/a.md', 'features/b.md', 'chats/session/log.md', 'features/graphrag/requirements.md'].entries()) {
    db.prepare('INSERT INTO documents VALUES(?,?,?,?,?)').run(i, 'docs/' + name, name.split('/')[0], name, 'rev-' + i);
    db.prepare('INSERT INTO chunks VALUES(?,?,?,?,?,?,?)').run(i, 0, 5, 7, 'Heading', 'Text ' + i, 'vec-' + i);
    db.prepare('INSERT INTO vectors VALUES(?,?)').run('vec-' + i, vector);
  }
  db.close();
  return root;
}

test('export preserves source database, scopes before graph, and refuses overwrite', () => {
  const root = fixture(), output = path.join(root, 'snapshot');
  const before = fs.readFileSync(path.join(root, '.bars', 'index.sqlite'));
  const result = run('export', root, output);
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(fs.readFileSync(path.join(root, '.bars', 'index.sqlite')), before);
  const manifest = json(path.join(output, 'manifest.json'));
  assert.equal(manifest.counts.chunks, 2);
  assert.deepEqual(manifest.files, ['features/a.md', 'features/b.md']);
  assert.equal(manifest.excluded_documents.length, 2);
  assert.equal(run('export', root, output).status, 1);
});

test('missing vectors fail before producing an apparently usable snapshot', () => {
  const root = fixture(), output = path.join(root, 'snapshot');
  const db = new DatabaseSync(path.join(root, '.bars', 'index.sqlite'));
  db.exec('DELETE FROM vectors'); db.close();
  assert.equal(run('export', root, output).status, 1);
  assert(!fs.existsSync(output));
});

test('analysis rejects snapshot drift, partial graphs and out-of-scope graph IDs', () => {
  const root = fixture(), output = path.join(root, 'snapshot');
  assert.equal(run('export', root, output).status, 0);
  const snapshot = json(path.join(output, 'snapshot.json'));
  const baseline = path.join(root, 'baseline.json'), graph = path.join(root, 'graph.json');
  write(baseline, [{ id: 'S1', query: 'Запрос', output: { results: snapshot.chunks.slice(0, 1) } }]);
  const complete = { total_nodes: 2, completed_nodes: 2, partial: false, graph: [{ source: 0, neighbors: [[1, 1]] }, { source: 1, neighbors: [[0, 1]] }] };
  write(graph, complete);
  const valid = run('analyze', output, baseline, graph);
  assert.equal(valid.status, 0, valid.stderr);
  assert.equal(JSON.parse(valid.stdout).cases[0].invalid, false);
  write(graph, { ...complete, partial: true });
  assert.equal(run('analyze', output, baseline, graph).status, 1);
  write(graph, { ...complete, graph: [{ source: 0, neighbors: [[2, 1]] }, complete.graph[1]] });
  assert.equal(run('analyze', output, baseline, graph).status, 1);
  write(graph, complete);
  fs.appendFileSync(path.join(output, 'snapshot.json'), '\n');
  assert.equal(run('analyze', output, baseline, graph).status, 1);
});

test('live hits with changed text are invalid, not scored as a retrieval success', () => {
  const root = fixture(), output = path.join(root, 'snapshot');
  assert.equal(run('export', root, output).status, 0);
  const snapshot = json(path.join(output, 'snapshot.json'));
  const baseline = path.join(root, 'baseline.json'), graph = path.join(root, 'graph.json');
  write(baseline, [{ id: 'S4', query: 'Запрос', output: { results: [{ ...snapshot.chunks[0], text: 'Другой текст' }] } }]);
  write(graph, { total_nodes: 2, completed_nodes: 2, partial: false, graph: [{ source: 0, neighbors: [[1, 1]] }, { source: 1, neighbors: [[0, 1]] }] });
  const result = run('analyze', output, baseline, graph);
  assert.equal(result.status, 0, result.stderr);
  const row = JSON.parse(result.stdout).cases[0];
  assert.equal(row.invalid, true);
  assert.equal(row.variants, undefined);
});
