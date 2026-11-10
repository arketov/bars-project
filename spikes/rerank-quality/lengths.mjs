// Token-only diagnostics, before threshold selection. No model inference.
import fs from 'node:fs';
import path from 'node:path';
import {createRequire} from 'node:module';
import assert from 'node:assert/strict';
import {read,write,sha} from './freeze.mjs';

const [directory,tokenizerFile,output='input-lengths.json']=process.argv.slice(2);
assert(directory&&tokenizerFile,'evidence tokenizer.json [new-output-name]');
assert(/^[a-z0-9-]+\.json$/.test(output));
const require=createRequire(path.resolve('docs/chats/2026-11-01-rtx-accelerators/fixture-node/package.json'));
const {Tokenizer}=require('@huggingface/tokenizers');
const configFile=path.join(path.dirname(tokenizerFile),'tokenizer_config.json');
const tokenizer=new Tokenizer(read(tokenizerFile),read(configFile));
const count=text=>tokenizer.encode(text,{add_special_tokens:false}).ids.length;
const inputFile=path.join(directory,'inputs.json'),protocol=read(path.join(directory,'protocol.json'));
assert.equal(sha(fs.readFileSync(inputFile)),protocol.inputs_sha256);
const lengths=read(inputFile).map(input=>({id:input.id,suite:input.suite,query_tokens:count(input.query),
  documents:input.documents.map((document,n)=>({tokens:count(document),target_offset:input.target_offsets?.[n]??null,
    target_prefix_tokens:input.target_offsets?.[n]==null?null:count(document.slice(0,input.target_offsets[n])),
    evidence_prefix_tokens:input.hits[n].relevant.map(g=>{
      const at=document.indexOf(input.gold[g].evidence);assert(at>=0,'Gold evidence present');
      return {gold:g,tokens:count(document.slice(0,at))};
    })}))}));
write(path.join(directory,output),{inputs_sha256:protocol.inputs_sha256,
  tokenizer_sha256:sha(fs.readFileSync(tokenizerFile)),tokenizer_config_sha256:sha(fs.readFileSync(configFile)),
  method:'untruncated XLM-R query/document counts; evidence positions use independent prefix tokenization',lengths});
