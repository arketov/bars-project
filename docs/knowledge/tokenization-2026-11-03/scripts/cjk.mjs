// cjk.mjs — CJK на node: что делает unicode61, trigram, биграммы, Intl.Segmenter (ICU), jieba (@node-rs/jieba).
// Корпус синтетический (CJK-документов в зарегистрированных проектах нет): 12 китайских и 12 японских коротких абзацев о поиске/настройках.
import { DatabaseSync } from 'node:sqlite';
import { req } from './lib.mjs';
const { Jieba } = req('@node-rs/jieba');
const jieba = new Jieba();
const DOCS = [
 ['zh', '搜索引擎使用倒排索引来快速查找文档。分词器把文本切成词语,再建立索引。'], ['zh', '全文检索需要中文分词,否则整句话会被当成一个词。'], ['zh', '配置文件中可以设置索引目录和模型路径。'], ['zh', '嵌入模型把文本转换成向量,用于语义搜索。'],
 ['zh', '重新排序模型对候选结果打分,提高前几名的准确率。'], ['zh', '数据库使用 SQLite 保存文档块和全文索引。'], ['zh', '错误日志记录了索引过程中出现的问题。'], ['zh', '用户可以在设置里修改搜索深度和阈值。'],
 ['zh', '文档按标题切分成块,每块单独建立索引。'], ['zh', '拼写错误会让关键词无法匹配,需要模糊搜索。'], ['zh', '项目目录下的 Markdown 文件会被自动索引。'], ['zh', '图谱显示文档之间的链接关系。'],
 ['ja', '検索エンジンは転置インデックスを使って文書を高速に探します。'], ['ja', '全文検索には日本語の形態素解析が必要です。'], ['ja', '設定ファイルでインデックスのディレクトリとモデルのパスを指定できます。'], ['ja', '埋め込みモデルはテキストをベクトルに変換して意味検索に使います。'],
 ['ja', 'リランクモデルは候補に点数を付けて上位の精度を高めます。'], ['ja', 'データベースは SQLite に文書のチャンクと全文インデックスを保存します。'], ['ja', 'エラーログにはインデックス作成中の問題が記録されます。'], ['ja', '設定で検索の深さとしきい値を変更できます。'],
 ['ja', '文書は見出しごとに分割され、それぞれにインデックスが作られます。'], ['ja', 'タイプミスがあるとキーワードが一致しないため、あいまい検索が必要です。'], ['ja', 'プロジェクト内の Markdown ファイルは自動的にインデックスされます。'], ['ja', 'グラフは文書間のリンク関係を表示します。'],
];
const Q = [ // [запрос, номер документа-ответа]
 ['倒排索引', 0], ['中文分词', 1], ['索引目录', 2], ['向量', 3], ['重新排序', 4], ['错误日志', 6], ['搜索深度', 7], ['模糊搜索', 9], ['链接关系', 11],
 ['転置インデックス', 12], ['形態素解析', 13], ['リランク', 16], ['エラーログ', 18], ['検索の深さ', 19], ['あいまい検索', 21], ['リンク関係', 23], ['設定', 14], ['ベクトル', 15],
];
const seg = { zh: new Intl.Segmenter('zh', { granularity: 'word' }), ja: new Intl.Segmenter('ja', { granularity: 'word' }) };
const isCjk = (s) => /[\u3040-\u30FF\u4E00-\u9FFF]/.test(s);
const tokenizers = {
  'unicode61 (как сейчас)': { tok: (t) => t, tokenize: "unicode61 remove_diacritics 2", q: (s) => `"${s}"` },
  'trigram FTS5': { tok: (t) => t, tokenize: 'trigram', q: (s) => (s.length >= 3 ? `"${s}"` : null) },
  'биграммы (свои)': { tok: (t) => t.replace(/[\u3040-\u30FF\u4E00-\u9FFF]+/g, (r) => { const o = []; if (r.length === 1) return r; for (let i = 0; i + 2 <= r.length; i++) o.push(r.slice(i, i + 2)); return ' ' + o.join(' ') + ' '; }), tokenize: 'unicode61', q: (s) => { const o = []; if (s.length === 1) o.push(s); for (let i = 0; i + 2 <= s.length; i++) o.push(s.slice(i, i + 2)); return o.map((x) => `"${x}"`).join(' AND '); } },
  'Intl.Segmenter (ICU)': { tok: (t, l) => [...seg[l].segment(t)].filter((x) => x.isWordLike).map((x) => x.segment).join(' '), tokenize: 'unicode61', q: (s, l) => [...seg[l].segment(s)].filter((x) => x.isWordLike).map((x) => `"${x.segment}"`).join(' AND ') },
  'jieba (только zh)': { tok: (t, l) => (l === 'zh' ? jieba.cut(t, true).join(' ') : t), tokenize: 'unicode61', q: (s, l) => (l === 'zh' ? jieba.cut(s, true).filter((x) => x.trim()).map((x) => `"${x}"`).join(' AND ') : `"${s}"`) },
};
for (const [name, T] of Object.entries(tokenizers)) {
  const d = new DatabaseSync(':memory:');
  d.exec(`create virtual table f using fts5(t, tokenize="${T.tokenize}")`);
  const ins = d.prepare('insert into f(rowid,t) values (?,?)');
  DOCS.forEach(([l, t], i) => ins.run(i, T.tok(t, l)));
  let hit = 0, any = 0, per = { zh: [0, 0], ja: [0, 0] };
  const miss = [];
  for (const [q, ans] of Q) {
    const l = DOCS[ans][0]; per[l][1]++;
    const m = T.q(T.tok(q, l).trim() === q ? q : q, l);
    const mm = name.startsWith('биграммы') || name.startsWith('Intl') || name.startsWith('jieba') ? T.q(q, l) : T.q(q, l);
    let rows = []; try { rows = mm ? d.prepare('select rowid from f where f match ? order by bm25(f) limit 5').all(mm) : []; } catch { rows = []; }
    if (rows.some((r) => r.rowid === ans)) { hit++; per[l][0]++; } else miss.push(q);
  }
  const toks = d.prepare("select 1").get() && (() => { d.exec("create virtual table v using fts5vocab(f,'row')"); return d.prepare('select count(*) n from v').get().n; })();
  console.log(name.padEnd(24), `найдено ${hit}/${Q.length} (zh ${per.zh[0]}/${per.zh[1]}, ja ${per.ja[0]}/${per.ja[1]})`, `термов в индексе ${toks}`, miss.length ? 'промахи: ' + miss.join(' ') : '');
}
// пример токенов
const ex = '全文检索需要中文分词,否则整句话会被当成一个词。';
console.log('jieba:', jieba.cut(ex, true).join('|'));
console.log('ICU zh:', [...seg.zh.segment(ex)].filter((x) => x.isWordLike).map((x) => x.segment).join('|'));
const exj = '全文検索には日本語の形態素解析が必要です。';
console.log('ICU ja:', [...seg.ja.segment(exj)].filter((x) => x.isWordLike).map((x) => x.segment).join('|'));
