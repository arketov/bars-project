import test from 'node:test';
import assert from 'node:assert/strict';
import {validateRows,validateEvents,summarize,registryDiagnostics} from './analyze.mjs';

const inputs=[{id:'p',gold:[{}],documents:['noise','answer'],hits:[{id:1,relevant:[]},{id:2,relevant:[0]}]},
  {id:'n',gold:[],documents:['noise'],hits:[{id:3,relevant:[]}]}];
const scores=[{id:'p',scores:[.2,.1]},{id:'n',scores:[.3]}];
test('a complete run cannot masquerade as another model or hide a guarded/crashed exit',()=>{
  const events=[{event:'start',name:'qwen',artifact:{name:'qwen'},inputs_sha256:'hash'},
    ...scores.map(r=>({event:'row',...r})),{event:'complete',name:'qwen',rows:2},{event:'exit',code:null,signal:'SIGTERM',guard:null}];
  assert.equal(validateEvents(inputs,events,'qwen','hash').size,2);
  assert.throws(()=>validateEvents(inputs,events,'other-model','hash'));
  assert.throws(()=>validateEvents(inputs,[...events,events[0]],'qwen','hash'));
  assert.throws(()=>validateEvents(inputs,[...events.slice(0,-1),{event:'exit',code:1,guard:null}],'qwen','hash'));
  assert.throws(()=>validateEvents(inputs,[...events.slice(0,-1),{event:'exit',code:null,signal:'SIGTERM',guard:'memory'}],'qwen','hash'));
});
test('missing, duplicate, unknown and malformed inference rows cannot pass as a run',()=>{
  for(const rows of [scores.slice(0,1),[scores[0],scores[0]],[scores[0],{id:'unknown',scores:[0]}],
    [scores[0],{id:'n',scores:[NaN]}],[scores[0],{id:'n',scores:[2]}],[scores[0],{id:'n',scores:[]}]]) {
    assert.throws(()=>validateRows(inputs,rows));
  }
});
test('head success cannot hide a rejected relevant tail',()=>{
  const input=[{id:'tail'}];
  const failure=registryDiagnostics(input,new Map([['tail',{scores:[.9,.01,.1]}]]),.3);
  const success=registryDiagnostics(input,new Map([['tail',{scores:[.9,.8,.1]}]]),.3);
  assert.equal(failure.head_kept,1);assert.equal(failure.tail_kept,0);assert.equal(failure.tail_above_filler,0);
  assert.equal(success.tail_kept,1);assert.equal(success.tail_above_filler,1);
});
test('threshold equality retained; losses and negative false outputs counted separately',()=>{
  const rows=validateRows(inputs,scores);
  const low=summarize(inputs,rows,.1),high=summarize(inputs,rows,.2);
  assert.equal(low.hit3,1);assert.equal(low.top1,0);assert.equal(low.mrr10,.5);
  assert.equal(low.false_nonempty,1);assert.equal(low.lost_top3,0);
  assert.equal(high.hit3,0);assert.equal(high.lost_top3,1);
  assert.equal(summarize(inputs,rows,.4).false_nonempty,0);
});
