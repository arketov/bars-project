import fs from 'node:fs';
import zlib from 'node:zlib';
import crypto from 'node:crypto';
import path from 'node:path';
import {execFileSync} from 'node:child_process';
const [root, destination] = process.argv.slice(2);
fs.mkdirSync(destination, {recursive: false});
const hash = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
for (const name of ['dataset.json', 'corpus.json', 'results.json']) {
  fs.writeFileSync(path.join(destination, `${name}.gz`), zlib.gzipSync(fs.readFileSync(path.join(root, name))), {flag:'wx'});
}
for (const name of ['analysis.json.gz', 'summary.json']) fs.copyFileSync(path.join(root,name),path.join(destination,name),fs.constants.COPYFILE_EXCL);
const sources = ['crates/bars-core/examples/graph_quality.rs', 'scripts/graph-quality-analyze.mjs', 'scripts/graph-quality-analyze.test.mjs', 'scripts/prepare-graph-rerank.mjs', 'crates/bars-core/src/search/hybrid.rs', 'crates/bars-core/src/graph/retrieval.rs', 'crates/bars-core/src/models/builtin_rerank.rs', 'crates/bars-core/src/models/rerank_artifact.rs'];
const inputs = ['gold-matched.json', 'index.sqlite.gz', 'query-vectors.json.gz', 'dataset.json.gz', 'freeze.json.gz'].map(n=>`docs/knowledge/graph-quality-2026-10-20/${n}`);
const artifactHashes = Object.fromEntries(fs.readdirSync(destination).map(n=>[n,hash(path.join(destination,n))]));
const manifest = {date:'2026-10-26', head:execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim(), sources:Object.fromEntries(sources.map(p=>[p,hash(p)])), inputs:Object.fromEntries(inputs.map(p=>[p,hash(p)])), artifactHashes, executable_sha256:hash('C:/bt/bars/debug/examples/graph_quality.exe'), limitations:['Known corpus; no independent holdout','Debug; fixed order; concurrent builds/tests; no speed acceptance','Core retrieval only; no IPC/native/installed acceptance'], scope:'66 frozen documents / 503 existing FP32 bge-m3 vectors; current graph; local FP16 DirectML rerank'};
fs.writeFileSync(path.join(destination,'provenance.json'),JSON.stringify(manifest,null,2),{flag:'wx'});
console.log(JSON.stringify(artifactHashes));
