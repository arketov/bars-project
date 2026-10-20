// Read-only source snapshot; no user profile or source index writes.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { DatabaseSync, backup } from 'node:sqlite';

if (process.argv.length !== 4) throw Error('root + NEW output required');
const [root, output] = process.argv.slice(2).map(p => path.resolve(p));
if (fs.existsSync(output)) throw Error('NEW output required');
fs.mkdirSync(output, { recursive: true });
const live = new DatabaseSync(path.join(root, '.bars/index.sqlite'), { readOnly: true });
await backup(live, path.join(output, 'index.sqlite'));
live.close();
const db = new DatabaseSync(path.join(output, 'index.sqlite'), { readOnly: true });
const docs = db.prepare('SELECT path,module,title,hash FROM documents ORDER BY path').all();
const selected = [], excluded = [];
for (const d of docs) {
  if (/^docs\/(chats|knowledge)\//.test(d.path) || /^docs\/features\/graphrag\//.test(d.path)
      || /(?:^|\/)(?:index|bars_instruction)\.md$/i.test(d.path)
      || /(?:graph|graphrag)\.md$/i.test(d.path)) {
    excluded.push({ path: d.path, reason: 'research/history/graph/instructions/index' });
    continue;
  }
  const target = path.join(output, d.path);
  if (!target.startsWith(output + path.sep)) throw Error('path escapes snapshot');
  try {
    const bytes = fs.readFileSync(path.join(root, d.path));
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, bytes, { flag: 'wx' });
    selected.push({ ...d, sha256: crypto.createHash('sha256').update(bytes).digest('hex') });
  } catch (e) { excluded.push({ path: d.path, reason: String(e.code ?? e) }); }
}
const manifest = { source_root: root, created_at: new Date().toISOString(), selected, excluded,
  meta: Object.fromEntries(db.prepare('SELECT key,value FROM meta').all().map(r => [r.key, r.value])) };
db.close();
fs.writeFileSync(path.join(output, 'freeze.json'), JSON.stringify(manifest, null, 2));
console.log(JSON.stringify({ selected: selected.length, excluded: excluded.length }));
