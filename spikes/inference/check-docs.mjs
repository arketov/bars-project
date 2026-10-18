import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const owned=['docs/knowledge/inference-speed.md','docs/knowledge/inference-devices.md','docs/knowledge/directml-performance.md','docs/knowledge/inference-libraries.md','docs/features/models/inference-plan.md','docs/knowledge/inference-2026-10-18/Index.md','docs/chats/2026-10-18-inference-design/handoff.md'];
const shared=['docs/knowledge/Index.md','docs/features/models/Index.md','docs/features/models/local-models.md','docs/features/settings/settings.md'];
for(const file of [...owned,...shared]){
  const text=fs.readFileSync(file,'utf8');
  assert(!/^(<<<<<<<|=======|>>>>>>>)\s*$/m.test(text),`merge marker ${file}`);
  if(owned.includes(file)){
    const title=text.match(/^---\r?\ntitle:\s*(.*?)\r?\n---/s)?.[1];
    const heading=text.match(/^# (.+)$/m)?.[1]?.trim();assert(title&&title.trim()===heading,`title/H1 ${file}`);
    assert(!/\]\((?:[^)]*\/)?PLAN\.md(?:#[^)]*)?\)/i.test(text),`forbidden plan link ${file}`);
  }
  for(const match of text.matchAll(/\]\(([^)]+)\)/g)){
    const target=match[1].split('#')[0];if(!target||/^[a-z][a-z0-9+.-]*:/i.test(target))continue;
    assert(fs.existsSync(path.resolve(path.dirname(file),decodeURIComponent(target))),`broken link ${file}: ${target}`);
  }
}
const dest='docs/knowledge/inference-2026-10-18',index=fs.readFileSync(`${dest}/Index.md`,'utf8');
const sums=JSON.parse(fs.readFileSync(`${dest}/checksums.json`));
for(const file of fs.readdirSync(dest)){
  if(file==='Index.md')continue;
  assert(index.includes(`](${file})`),`evidence not indexed: ${file}`);
  if(file==='checksums.json')continue;
  assert.equal(crypto.createHash('sha256').update(fs.readFileSync(`${dest}/${file}`)).digest('hex'),sums[file],`checksum ${file}`);
}
console.log(`${owned.length+shared.length} documents: titles/links/markers OK; ${Object.keys(sums).length} evidence SHA256 OK`);
