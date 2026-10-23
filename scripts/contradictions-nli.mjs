// Пункт 3.8.1: NLI-проба. Нужны npm i @huggingface/transformers@3.7.5 и файлы модели MoritzLaurer/mDeBERTa-v3-base-xnli-multilingual-nli-2mil7 в ./models/ рядом со скриптом (вне репозитория).
// вход: JSON-массив {a,b}; выход: JSON-массив {pc,pe,pn} — максимум противоречия в обоих направлениях
import fs from 'node:fs';
import { env, AutoTokenizer, AutoModelForSequenceClassification } from '@huggingface/transformers';
env.allowRemoteModels = false; env.localModelPath = new URL('./models/', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1');
const id = 'MoritzLaurer/mDeBERTa-v3-base-xnli-multilingual-nli-2mil7';
const tok = await AutoTokenizer.from_pretrained(id);
const model = await AutoModelForSequenceClassification.from_pretrained(id, { dtype: 'q8' });
const softmax = l => { const m = Math.max(...l); const e = l.map(x => Math.exp(x - m)); const s = e.reduce((a, b) => a + b); return e.map(x => x / s); };
async function one(a, b) {
  const inp = tok([a], { text_pair: [b], truncation: true, max_length: 256, padding: true });
  const out = await model(inp);
  return softmax(Array.from(out.logits.data)); // entailment, neutral, contradiction
}
const [inFile, outFile] = process.argv.slice(2);
const pairs = JSON.parse(fs.readFileSync(inFile, 'utf8'));
const res = []; const t0 = Date.now();
for (const [i, p] of pairs.entries()) {
  const f = await one(p.a, p.b), r = await one(p.b, p.a);
  res.push({ pc: Math.max(f[2], r[2]), pcf: f[2], pcr: r[2], pe: Math.max(f[0], r[0]), pn: Math.min(f[1], r[1]) });
  if (i % 50 === 49) console.error(i + 1, Date.now() - t0, 'ms');
}
fs.writeFileSync(outFile, JSON.stringify({ ms: Date.now() - t0, n: pairs.length, res }));
console.log('done', pairs.length, Date.now() - t0, 'ms');
