// Preserve only completed initial runs; these are preliminary, not final timings.
import fs from 'node:fs';
import crypto from 'node:crypto';
import zlib from 'node:zlib';
const dest='docs/knowledge/inference-2026-10-18';
fs.mkdirSync(dest,{recursive:true});
const sources={
  'baseline.json':'runs/screen-partial.json',
  'cpu16-monitor.json':'runs/screen/cpu-t16-b4.monitor.json',
  'dml-monitor.json':'runs/screen/dml-b4.monitor.json',
  'profile-cpu.json':'runs/profile-cpu-t16-b4/summary.json',
  'profile-dml.json':'runs/profile-dml-b4/summary.json',
  'hardware.json':'provenance/hardware.json',
  'holdout-frozen.json':'provenance/holdout-frozen.json',
  'memory-blocker.json':'provenance/memory-blocker.json',
  'cpu1-invalidated.json':'runs/screen/cpu-t1-b4/invalidated.json',
  'recovery-check.json':'runs/recovery-check.json',
  'recovery-dml-monitor.json':'runs/recovery/dml-b4.monitor.json',
};
for(const [name,source] of Object.entries(sources)) {
  const bytes=fs.readFileSync(`spikes/inference/${source}`);
  JSON.parse(bytes);
  fs.writeFileSync(`${dest}/${name}`,bytes);
}
for(const [label,run] of [['cpu16','screen/cpu-t16-b4'],['dml','screen/dml-b4'],['recovery-dml','recovery/dml-b4']]) {
  const root=`spikes/inference/runs/${run}`;
  const complete=JSON.parse(fs.readFileSync(`${root}/complete.json`));
  const bytes=fs.readFileSync(`${root}/rows.jsonl`);
  const rows=bytes.toString().trim().split(/\r?\n/).map(JSON.parse);
  if(rows.length!==30)throw Error(`Unexpected initial count: ${label}`);
  fs.writeFileSync(`${dest}/${label}-rows.jsonl.gz`,zlib.gzipSync(bytes));
  // Retain provenance and completion metadata next to the compressed rows.
  fs.writeFileSync(`${dest}/${label}-run.json`,JSON.stringify({
    config:JSON.parse(fs.readFileSync(`${root}/config.json`)),
    session:JSON.parse(fs.readFileSync(`${root}/session.json`)),complete,
  },null,2));
}
for(const variant of ['fp16','quantized']) {
  const name=`model-${variant}.json`;
  fs.copyFileSync(`spikes/inference/data/models/ranged-model_${variant}.onnx.verified.json`,`${dest}/${name}`);
}
const sums={};
for(const name of fs.readdirSync(dest).sort()) {
  if(name==='checksums.json'||name.endsWith('.md'))continue;
  sums[name]=crypto.createHash('sha256').update(fs.readFileSync(`${dest}/${name}`)).digest('hex');
}
fs.writeFileSync(`${dest}/checksums.json`,JSON.stringify(sums,null,2)+'\n');
console.log(`Preserved ${Object.keys(sums).length} evidence files`);
