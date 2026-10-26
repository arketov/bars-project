// Read-only SQL comparison; one command per stdin line, one JSON result per stdout line.
const { DatabaseSync } = require('node:sqlite');
const { createInterface } = require('node:readline');
const { createHash } = require('node:crypto');
const queries = {
  lookup: 'SELECT c.id,v.vector FROM chunks c JOIN documents d ON d.id=c.document_id JOIN vectors v ON v.embed_hash=c.embed_hash WHERE 1',
  sequential: 'SELECT c.id,v.vector FROM vectors v CROSS JOIN chunks c ON c.embed_hash=v.embed_hash CROSS JOIN documents d ON d.id=c.document_id WHERE 1',
  lookup_keys: 'SELECT DISTINCT c.embed_hash FROM chunks c JOIN vectors v ON v.embed_hash=c.embed_hash ORDER BY c.embed_hash',
  sequential_keys: 'SELECT DISTINCT v.embed_hash FROM vectors v CROSS JOIN chunks c ON c.embed_hash=v.embed_hash ORDER BY v.embed_hash',
};
createInterface({ input: process.stdin }).on('line', mode => {
  const db = new DatabaseSync(process.argv[2], { readOnly: true });
  const start = performance.now();
  let rows = 0, bytes = 0, checksum = 0;
  const hashes = [];
  for (const row of db.prepare(queries[mode]).iterate()) {
    rows++; bytes += row.vector?.length ?? 0; checksum += row.id ?? 0;
    hashes.push(row.vector
      ? `${row.id}:${createHash('sha256').update(row.vector).digest('hex')}`
      : row.embed_hash);
  }
  const result = { mode, rows, bytes, checksum, ms: performance.now() - start,
    digest: createHash('sha256').update(hashes.sort().join('\n')).digest('hex'),
    plan: db.prepare(`EXPLAIN QUERY PLAN ${queries[mode]}`).all().map(row => row.detail) };
  db.close();
  console.log(JSON.stringify(result));
});
