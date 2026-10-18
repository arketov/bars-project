// Только собственные копии индексов и hardlink-кэш неизменяемых весов; рабочий профиль не используется.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {DatabaseSync, backup} from 'node:sqlite';

const root=process.cwd();
const destination=path.resolve(process.argv[2]??'spikes/memory/embed-profiles/core-v1');
assert(destination.startsWith(path.join(root,'spikes','memory','embed-profiles')+path.sep));
assert(!fs.existsSync(destination),'choose a new fixture directory');
const revision='4de13258303883538bd53b696b452bf8099f0858';
const models=path.join(destination,'models');
fs.mkdirSync(models,{recursive:true});
for(const name of ['models--BAAI--bge-m3','models--rozgo--bge-reranker-v2-m3','models--onnx-community--bge-reranker-v2-m3-ONNX']) {
  const from=path.resolve('spikes/inference/data/product-models',name);
  assert(fs.statSync(from).isDirectory());
  fs.symlinkSync(from,path.join(models,name),'junction');
}
const repo=path.join(models,'models--Xenova--bge-m3');
const snapshot=path.join(repo,'snapshots',revision);
fs.mkdirSync(path.join(snapshot,'onnx'),{recursive:true});
fs.mkdirSync(path.join(repo,'refs'),{recursive:true});
fs.writeFileSync(path.join(repo,'refs',revision),revision,{flag:'wx'});
for(const name of ['config.json','tokenizer.json','special_tokens_map.json','tokenizer_config.json'])
  fs.linkSync(path.resolve('spikes/inference/data/embed-models',name),path.join(snapshot,name));
for(const [profile,file] of [['fp16','model_fp16.onnx'],['int8','model_quantized.onnx']]) {
  fs.linkSync(path.resolve('spikes/inference/data/embed-models','ranged-'+file),path.join(snapshot,'onnx',file));
  const source=path.resolve('spikes/inference/runs/index-validation',profile==='fp16'?'embed-fp16-dml-b8':'embed-int8-cpu16-b1','full-data');
  const data=path.join(destination,profile);
  fs.mkdirSync(data);
  for(const name of ['inputs.json','cases.json']) fs.copyFileSync(path.join(source,name),path.join(data,name),fs.constants.COPYFILE_EXCL);
  const input=new DatabaseSync(path.join(source,'index.sqlite'),{readOnly:true});
  await backup(input,path.join(data,'index.sqlite')); input.close();
  const db=new DatabaseSync(path.join(data,'index.sqlite'));
  const fp={source:'builtin',model:`BAAI/bge-m3@${profile}:${revision}`,dim:1024,doc_prefix:''};
  const update=db.prepare('UPDATE meta SET value=? WHERE key=?');
  for(const [key,value] of [['model_source',fp.source],['model_id',fp.model],['model_dim','1024'],['model_doc_prefix','']])
    assert.equal(update.run(value,key).changes,1);
  db.close();
  const manifest=JSON.parse(fs.readFileSync(path.join(source,'manifest.json'),'utf8'));
  manifest.embedding_profile_origin={source:path.relative(root,source).replaceAll('\\','/'),original_index_blake3:manifest.index_blake3,
    method:'SQLite backup; metadata adapted to core identity of identical pinned export; not a new product indexing pass'};
  manifest.builtin_embedding_profile=profile;
  const result=spawnSync(path.join(process.env.LOCALAPPDATA,'bars/target/release/examples/bench_inference.exe'),['digest',path.join(data,'index.sqlite')],{encoding:'utf8'});
  assert.equal(result.status,0,result.stderr);
  manifest.index_blake3=result.stdout.trim();
  fs.writeFileSync(path.join(data,'manifest.json'),JSON.stringify(manifest,null,2)+'\n',{flag:'wx'});
  fs.writeFileSync(path.join(data,'core-fingerprint.json'),JSON.stringify(fp,null,2)+'\n',{flag:'wx'});
}
console.log(JSON.stringify({destination:path.relative(root,destination).replaceAll('\\','/'),models:path.relative(root,models).replaceAll('\\','/')}));
