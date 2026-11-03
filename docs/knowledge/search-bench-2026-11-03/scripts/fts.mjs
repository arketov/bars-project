// fts.mjs <проект> [selftest] — полнотекстовые варианты на копии индекса, правило MATCH как в hybrid.rs::fts_query
import { DatabaseSync } from 'node:sqlite';
import fs from 'node:fs';
import { makeStemmer, stemRu, words } from './stem.mjs';

const home = process.env.USERPROFILE + '/.bars/bench/';
export function openProject(proj) {
  const d = new DatabaseSync(home + proj + '/index.sqlite', { readOnly: true });
  // выучить слово→основа выравниванием текста чанка и его стеммированной копии в chunks_fts_content
  const learned = new Map();
  let agree = 0, total = 0, mism = 0;
  const rows = d.prepare('select c.id, c.heading_chain h, c.text t, f.c0, f.c1 from chunks c join chunks_fts_content f on f.id=c.id').all();
  for (const r of rows) {
    for (const [src, st] of [[r.h, r.c0], [r.t, r.c1]]) {
      const a = words(src), b = st.split(' ').filter(Boolean);
      if (a.length !== b.length) { mism++; continue; }
      for (let i = 0; i < a.length; i++) {
        const l = a[i].toLowerCase();
        if (!learned.has(l)) learned.set(l, b[i]);
      }
    }
  }
  // самопроверка собственного русского стеммера на выученных парах
  for (const [w, s] of learned) if (/^[а-яё]+$/.test(w)) { total++; if (stemRu(w) === s) agree++; }
  return { d, learned, stats: { chunks: rows.length, mismatchedFields: mism, ruWords: total, ruAgree: agree } };
}

const isIdent = (raw) => /[_./]/.test(raw.replace(/^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu, ''));
export function ftsQuery(query, stem, mode = 'or') {
  const phrases = [];
  const terms = [];
  for (const raw of query.split(/\s+/).filter(Boolean)) {
    const st = stem(raw);
    if (st.length < 2) continue;
    const p = '"' + st.join(' ') + '"';
    if (!phrases.includes(p)) phrases.push(p);
  }
  for (const t of stem(query)) { const q = '"' + t + '"'; if (!terms.includes(q)) terms.push(q); }
  if (mode === 'and') return terms.length ? terms.join(' AND ') : null;
  const parts = [...phrases];
  for (const t of terms) if (!parts.includes(t)) parts.push(t);
  return parts.length ? parts.join(' OR ') : null;
}

export function runFts(p, query, variant, limit = 30) {
  const stem = makeStemmer(p.learned);
  const q = ftsQuery(query, stem, variant === 'and' ? 'and' : 'or');
  if (!q) return [];
  const rank = variant === 'head5' ? 'bm25(chunks_fts,5,1)' : 'bm25(chunks_fts)';
  try {
    return p.d.prepare(`select c.id, d.path, c.first_line, c.last_line, c.heading_chain h, c.text t, d.module m from chunks_fts join chunks c on c.id=chunks_fts.rowid join documents d on d.id=c.document_id where chunks_fts match ? order by ${rank} limit ?`).all(q, limit);
  } catch (e) { return []; }
}

if (process.argv[2] && process.argv[3] === 'selftest') {
  const p = openProject(process.argv[2]);
  console.log(process.argv[2], p.stats, 'совпадение стеммера', (100 * p.stats.ruAgree / p.stats.ruWords).toFixed(1) + '%');
}
