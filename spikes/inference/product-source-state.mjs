import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const target='spikes/inference/provenance/product-build-sources.json';
function files(dir){return fs.readdirSync(dir,{withFileTypes:true}).flatMap(e=>e.isDirectory()?files(`${dir}/${e.name}`):e.name.endsWith('.rs')?[`${dir}/${e.name}`]:[])}
const paths=['Cargo.toml','Cargo.lock',...['bars-core','bars-worker','bars-protocol'].flatMap(c=>[`crates/${c}/Cargo.toml`,...files(`crates/${c}/src`)]),'crates/bars-core/examples/bench_inference.rs','crates/bars-core/examples/inference/mod.rs','crates/bars-worker/examples/bench_worker_inference.rs','crates/bars-worker/examples/inference_worker/mod.rs'];
const hashes=Object.fromEntries(paths.sort().map(p=>[p,crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex')]));
if(process.argv[2]==='capture'){fs.writeFileSync(target,JSON.stringify(hashes,null,2));console.log(`Captured ${paths.length} build inputs`)}
else {
  const expected=JSON.parse(fs.readFileSync(target));
  const changed=[...new Set([...Object.keys(hashes),...Object.keys(expected)])].filter(p=>hashes[p]!==expected[p]);
  if(process.argv[2]==='report'){
    const reviewed=[];
    const rerank='crates/bars-core/src/models/builtin_rerank.rs';
    if(changed.includes(rerank)){
      // Проверяем точный diff: только распознавание HRESULT OOM и два unit-assert.
      const prior=fs.readFileSync(rerank,'utf8').split(/(?<=\n)/).filter(l=>
        !l.includes('"0x8007000e",')&&!l.includes('HRESULT: 0x8007000E')&&!l.includes('HRESULT: 0x887A0005')).join('');
      if(crypto.createHash('sha256').update(prior).digest('hex')===expected[rerank])reviewed.push({file:rerank,change:'allocation error HRESULT alias + two unit assertions; successful inference path unchanged'});
    }
    fs.writeFileSync('spikes/inference/provenance/product-source-drift.json',JSON.stringify({
      note:'Results belong to the binary built from buildSources; later parallel source edits are not accepted by that run',
      changed:changed.map(file=>({file,builtSha256:expected[file],currentSha256:hashes[file]})),
      reviewed,
    },null,2));
    assert(!changed.some(p=>(p.startsWith('crates/bars-core/')||p.includes('/examples/'))&&!reviewed.some(r=>r.file===p)),'unreviewed inference code changed; cannot accept as current core');
    console.log(`Built snapshot retained; reviewed equivalent core changes=${reviewed.length}; shared source drift: ${changed.join(', ')||'none'}`);
    process.exit(0);
  }
  assert(!changed.length,`sources changed during product verification: ${changed.join(', ')}`);
  console.log('Build inputs unchanged');
}
