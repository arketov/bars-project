// lang.mjs — определение языка: письменность (правило), tinyld, franc-min на словах, фразах и смешанном тексте.
// whatlang и lingua-rs (Rust) не запускались: сборка запрещена, оценка по документации.
import { req, loadProject, PROJECTS } from './lib.mjs';
const tinyld = req('tinyld'), franc = req('franc-min');
// франк возвращает ISO 639-3
const m3to1 = { rus: 'ru', ukr: 'uk', eng: 'en', deu: 'de', fra: 'fr', spa: 'es', cmn: 'zh', jpn: 'ja', bel: 'be', bul: 'bg', srp: 'sr', mkd: 'mk', und: '??' };
const S = [ // [язык, текст]
  ['ru', 'как настроить поиск по документам'], ['ru', 'застревание процесса при индексации'], ['ru', 'сертификат соответствия и декларация'], ['ru', 'режим работы воркера'], ['ru', 'настройки'], ['ru', 'индексация'], ['ru', 'опечатка'], ['ru', 'декларирование товаров'], ['ru', 'таможенный союз'], ['ru', 'ошибка при запуске приложения после обновления'],
  ['uk', 'як налаштувати пошук по документах'], ['uk', 'зависання процесу під час індексації'], ['uk', 'сертифікат відповідності та декларація'], ['uk', 'налаштування'], ['uk', 'індексація'], ['uk', 'помилка запуску програми після оновлення'], ['uk', 'режим роботи воркера'], ['uk', 'митний союз'], ['uk', 'перевірка'], ['uk', 'їжак'],
  ['en', 'how to configure document search'], ['en', 'process hangs during indexing'], ['en', 'settings'], ['en', 'indexing'], ['en', 'worker mode'], ['en', 'declaration of conformity'], ['en', 'startup error after update'], ['en', 'tokenizer'], ['en', 'typo'], ['en', 'search engine'],
  ['de', 'wie konfiguriere ich die dokumentensuche'], ['de', 'einstellungen'], ['de', 'fehler beim start'], ['de', 'suchmaschine'], ['de', 'konformitätserklärung'],
  ['fr', 'comment configurer la recherche de documents'], ['fr', 'paramètres'], ['fr', 'erreur au démarrage'], ['fr', 'moteur de recherche'], ['fr', 'déclaration de conformité'],
  ['zh', '如何配置文档搜索'], ['zh', '索引错误'], ['zh', '设置'], ['ja', 'ドキュメント検索の設定方法'], ['ja', 'インデックスのエラー'], ['ja', '設定'],
];
const MIX = [ // смешанный текст: язык — основной
  ['ru', 'как настроить embed модель и rerank в поиске'], ['ru', 'ошибка model_loading при вызове MCP'], ['ru', 'индексатор падает на SQLite FTS5 tokenizer'], ['ru', 'сборка через cargo build и tauri bundle'], ['ru', 'ОПИ и КИИ в нормативке'], ['en', 'the Russian word настройки means settings'], ['en', 'search for декларация in the index'],
];
function script(t) {
  let cy = 0, la = 0, cjk = 0, kana = 0, ukr = 0, rusOnly = 0;
  for (const ch of t) {
    if (/[\u0400-\u04FF]/.test(ch)) { cy++; if ('іїєґІЇЄҐ'.includes(ch)) ukr++; if ('ыэъЫЭЪ'.includes(ch)) rusOnly++; }
    else if (/[a-zA-Z\u00C0-\u024F]/.test(ch)) la++;
    else if (/[\u3040-\u30FF]/.test(ch)) kana++;
    else if (/[\u4E00-\u9FFF]/.test(ch)) cjk++;
  }
  if (kana) return 'ja';
  if (cjk && cjk >= cy + la) return 'zh';
  if (cy >= la && cy) return ukr > rusOnly ? 'uk' : rusOnly ? 'ru' : 'cyr?'; // кириллица без различающих букв: ru/uk не различить
  if (la) return 'lat?';
  return '??';
}
const methods = {
  'письменность+буквы': (t) => script(t),
  tinyld: (t) => tinyld.detect(t) || '??',
  'franc-min': (t) => m3to1[franc.franc(t, { minLength: 1 })] || franc.franc(t, { minLength: 1 }),
};
function run(set, label) {
  console.log('\n' + label);
  for (const [name, f] of Object.entries(methods)) {
    const t0 = process.hrtime.bigint(); let ok = 0; const bad = [];
    for (const [l, t] of set) { const r = f(t); const good = r === l || (name.startsWith('письм') && ((l === 'en' || l === 'de' || l === 'fr') && r === 'lat?' || (l === 'ru' && r === 'cyr?'))); if (good) ok++; else bad.push(`${l}→${r}:${t.slice(0, 22)}`); }
    const us = Number(process.hrtime.bigint() - t0) / 1000 / set.length;
    console.log(name.padEnd(22), `${ok}/${set.length}`, `${us.toFixed(0)} мкс/текст`, bad.length ? ' ошибки: ' + bad.slice(0, 8).join('; ') : '');
  }
}
const words1 = S.filter(([, t]) => !t.includes(' ')), words2 = S.filter(([, t]) => t.split(' ').length >= 2);
run(words1, 'одно слово (' + words1.length + ')');
run(words2, 'фраза из 2+ слов (' + words2.length + ')');
run(MIX, 'смешанные фразы (ru/en в одной строке, ожидается основной язык)');
// доля слов корпуса: письменность по словам трёх проектов
for (const p of ['bars-project', 'agaugu']) {
  const { chunks } = loadProject(p);
  const cnt = {};
  for (const c of chunks.slice(0, 3000)) for (const w of c.t.split(/[^\p{L}\p{N}]+/u).filter(Boolean)) { const s = /^\d+$/.test(w) ? 'num' : /[\u0400-\u04FF]/.test(w) && /[a-zA-Z]/.test(w) ? 'mix-script' : /[\u0400-\u04FF]/.test(w) ? 'cyr' : /[a-zA-Z]/.test(w) ? 'lat' : 'other'; cnt[s] = (cnt[s] || 0) + 1; }
  console.log(p, JSON.stringify(cnt));
}
// определение языка документа: tinyld на первых 2000 знаках текста каждого документа
for (const p of PROJECTS) {
  const { chunks } = loadProject(p);
  const docs = new Map();
  for (const c of chunks) { if (!docs.has(c.path)) docs.set(c.path, c.t); }
  const cnt = {};
  for (const t of docs.values()) { const l = tinyld.detect(t.slice(0, 2000)) || '??'; cnt[l] = (cnt[l] || 0) + 1; }
  console.log(p, 'документов', docs.size, JSON.stringify(cnt));
}
