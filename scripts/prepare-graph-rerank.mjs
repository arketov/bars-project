// Restore the existing research corpus only; never open a working .bars index.
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
const [archive, destination] = process.argv.slice(2);
if (!archive || !destination) throw Error('archive destination required');
fs.mkdirSync(destination, {recursive: false});
for (const name of ['index.sqlite', 'freeze.json', 'query-vectors.json']) {
  fs.writeFileSync(path.join(destination, name), zlib.gunzipSync(fs.readFileSync(path.join(archive, `${name}.gz`))), {flag: 'wx'});
}
const data = JSON.parse(zlib.gunzipSync(fs.readFileSync(path.join(archive, 'dataset.json.gz'))));
const root = path.resolve(destination);
for (const source of data.sources) {
  const target = path.resolve(root, source.meta.path);
  if (!target.startsWith(root + path.sep)) throw Error('source outside snapshot');
  fs.mkdirSync(path.dirname(target), {recursive: true});
  fs.writeFileSync(target, source.text, {flag: 'wx'});
}
console.log(`Restored ${data.sources.length} sources; original query vectors retained`);
