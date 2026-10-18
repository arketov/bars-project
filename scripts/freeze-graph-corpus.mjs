// Фиксация development-результата; не превращает его в независимый holdout.
import {readFile, writeFile, mkdir, stat} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {gzipSync} from 'node:zlib';
import {resolve, join} from 'node:path';

const [input, output] = process.argv.slice(2);
if (!input || !output) throw Error('freeze-graph-corpus <probe-output> <new-evidence-folder>');
const source = resolve(input), destination = resolve(output);
try { await stat(destination); throw Error('Папка результата уже существует'); }
catch (error) { if (error.code !== 'ENOENT') throw error; }
const files = await Promise.all(['dataset.json','measurement.json','process.json'].map(async name => ({name, bytes:await readFile(join(source,name))})));
const dataset = JSON.parse(files[0].bytes), measurement = JSON.parse(files[1].bytes);
if (dataset.namespace !== 'bars-project:corpus:2026-10-18') throw Error('Нужен обезличенный namespace corpus probe');
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const nodes = new Map(dataset.nodes.map(node => [node.reference.id,node]));
const evidence = new Map(dataset.evidence.map(proof => [proof.id,proof]));
const sources = new Map(dataset.sources.map(document => [document.meta.reference.id,document.meta]));
const assertions = dataset.edges.filter(edge=>edge.origin==='explicit_rule').map(edge=>({
    id:edge.id, subject:nodes.get(edge.from)?.label, predicate:edge.predicate, polarity:edge.polarity,
    object:nodes.get(edge.to)?.label, qualifiers:edge.qualifiers,
    evidence:edge.evidence.map(id=>{const proof=evidence.get(id);const path=sources.get(proof.document.id)?.docs_path;if(!path)throw Error('Неразрешённое основание '+id);return {...proof,path};})
}));
const manifest = {kind:'development-observation', independent_holdout:false,
    method:dataset.evidence.find(proof=>proof.method?.includes('explicit-markdown'))?.method ?? dataset.semantic_state,
    snapshot:dataset.snapshot, embedding:measurement.meta, vectors_recomputed:false,
    files:Object.fromEntries(files.map(({name,bytes})=>[name,{sha256:sha256(bytes),bytes:bytes.length}])),
    sources:dataset.sources.map(({meta})=>({path:meta.docs_path,hash:meta.hash,reference:meta.reference})),
    exclusions:measurement.exclusions};
await mkdir(destination,{recursive:true});
for (const {name,bytes} of files) await writeFile(join(destination,name+'.gz'),gzipSync(bytes),{flag:'wx'});
await writeFile(join(destination,'manifest.json'),JSON.stringify(manifest,null,2)+'\n',{flag:'wx'});
await writeFile(join(destination,'assertions.json'),JSON.stringify(assertions,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({documents:manifest.sources.length,assertions:assertions.length,output:destination}));
