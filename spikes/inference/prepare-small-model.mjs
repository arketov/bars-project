import fs from 'node:fs';
import crypto from 'node:crypto';
const root='spikes/inference/data/minilm';
const repository='cross-encoder/mmarco-mMiniLMv2-L12-H384-v1';
if(fs.existsSync(`${root}/source.json`))throw Error('Source already frozen');
const metadata=await (await fetch(`https://huggingface.co/api/models/${repository}`)).json();
if(!/^[a-f0-9]{40}$/.test(metadata.sha))throw Error('Missing revision');
const revision=metadata.sha;
fs.mkdirSync(root,{recursive:true});
const hashes={};
for(const name of ['config.json','tokenizer.json','tokenizer_config.json','special_tokens_map.json']) {
  const response=await fetch(`https://huggingface.co/${repository}/resolve/${revision}/${name}`);
  if(!response.ok)throw Error(`${name}: ${response.status}`);
  const bytes=Buffer.from(await response.arrayBuffer());
  JSON.parse(bytes);
  fs.writeFileSync(`${root}/${name}`,bytes);
  hashes[name]=crypto.createHash('sha256').update(bytes).digest('hex');
}
fs.writeFileSync(`${root}/source.json`,JSON.stringify({repository,revision,files:['onnx/model.onnx'],tokenizerSha256:hashes},null,2));
console.log(`Frozen ${repository} ${revision}`);
