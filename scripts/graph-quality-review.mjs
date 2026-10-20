// Post-prediction manual census. No product rule is tuned against these labels.
import fs from 'node:fs';
import path from 'node:path';
const [root,output]=process.argv.slice(2);
const d=JSON.parse(fs.readFileSync(`${root}/dataset.json`));
const nodes=new Map(d.nodes.map(n=>[n.reference.id,n]));
const evidence=new Map(d.evidence.map(e=>[e.id,e]));
const sources=new Map(d.sources.map(s=>[s.meta.reference.id,s]));
const semantic=d.edges.filter(e=>['depends_on','uses','implements','verifies','supersedes','requires'].includes(e.predicate));
// Confirmed error: wrong participant/polarity, swallowed condition, or malformed code identifier.
// Unresolved: quote is literal but canonical participant/referent has not been grounded.
const judgements={
 '1974fd65':['supported','Отрицание замены меню передано буквально.'],
 '1be094fc':['unresolved','Наречие заново включено в subject; цель не разрешена в объект.'],
 '28b8951b':['error','Subject=не и positive требует: потерян действующий субъект и отрицание.'],
 '47f6c1ce':['unresolved','Новый результат и документ в той же колонке требуют разрешения контекста.'],
 '518f0879':['supported','Отказ учёта не заменяет результат инструмента; отрицание сохранено.'],
 '65afe50b':['error','Без драйвера Vulkan поглощено subject; условие отсутствует в qualifiers.'],
 '759ae14c':['supported','Новая правка заменяет ждущую запись того же документа; явное правило очереди.'],
 '7eced287':['supported','Буквальный факт bars использует две модели; типы моделей не извлечены.'],
 '8bd41611':['unresolved','Этот отчёт не разрешён в именованную сущность.'],
 '8d9a070b':['error','Обрезан закрывающий backtick в имени unregister; итоговый участник не является корректным исходным идентификатором.'],
 '8f51428a':['supported','Отрицание замены нативной приёмки этапа 32 сохранено.'],
 '903fe4a4':['unresolved','Тот же проверенный путь не разрешён в явный объект.'],
 '9b9cf930':['unresolved','Наречие снова включено в subject, внимания не именованный объект.'],
 '9cfae635':['supported','MCP использует ID допущенной попытки и epoch; буквальное утверждение.'],
 'a3d15933':['supported','Сбор не требует открытого окна; отрицание сохранено, subject локален документу.'],
 'a89bfe72':['unresolved','В названии subject потеряна парная кавычка; устойчивое имя не подтверждено.'],
 'aaea7d0f':['error','Обрезан закрывающий backtick метода embed; object включает соседнее пояснение после двоеточия.'],
 'b4440de5':['unresolved','В object поглощено отдельное пояснение --input; чтение не разрешено в компонент.'],
 'bb59639d':['unresolved','Отметка требует разрешения к типу проверенного кэша.'],
 'bca3ca5d':['supported','Первое скачивание требует сети; буквальная зависимость.'],
 'd96b4402':['unresolved','Способ и период проверки поглощены object; квалификаторы не отделены.'],
};
const assertions=semantic.map(e=>{
  const j=judgements[e.id.slice(0,8)];
  if(!j) throw Error(`new assertion requires review: ${e.id}`);
  return {...e,subject:nodes.get(e.from).label,object:nodes.get(e.to).label,verdict:j[0],reason:j[1],
    proof:e.evidence.map(id=>{const ev=evidence.get(id);return {path:sources.get(ev.document.id).meta.docs_path,quote:ev.quote,span:ev.span}})};
});
if(assertions.length!==Object.keys(judgements).length) throw Error('review must cover the full census');
const counts=Object.fromEntries(['supported','unresolved','error'].map(v=>[v,assertions.filter(a=>a.verdict===v).length]));
const selected=new Set(d.sources.map(s=>s.meta.docs_path));
const manifest=JSON.parse(fs.readFileSync(`${root}/freeze.json`));
const excluded=new Set(manifest.excluded.map(e=>e.path.replace(/^docs\//,'')));
const links=d.broken_links.map(l=>{
 const ev=evidence.get(l.evidence),s=sources.get(ev.document.id);
 const [target,anchor]=decodeURIComponent(l.target).split('#');
 const resolved=target?path.posix.normalize(path.posix.join(path.posix.dirname(s.meta.docs_path),target)):s.meta.docs_path;
 const classification=selected.has(resolved)?'selected_target_anchor_or_parser_review':excluded.has(resolved)?'excluded_from_snapshot':'not_indexed_in_original_snapshot';
 return {...l,source_path:s.meta.docs_path,resolved_target:resolved,anchor,classification};
});
const semantics={entities:d.nodes.filter(n=>n.reference.kind==='entity').length,assertions:semantic.length,
  with_conditions:semantic.filter(e=>Object.keys(e.qualifiers).length).length,
  cross_file_semantic_edges:semantic.filter(e=>nodes.get(e.from).namespace!==nodes.get(e.to).namespace).length,
  two_step_semantic_chains:semantic.reduce((n,e)=>n+semantic.filter(f=>f.from===e.to).length,0),
  roles:d.nodes.reduce((a,n)=>{for(const r of n.roles)a[r]=(a[r]??0)+1;return a},{}),
  verdict_counts:counts,literal_tuple_precision_lower:counts.supported/assertions.length,
  literal_tuple_precision_upper:(counts.supported+counts.unresolved)/assertions.length};
const report={reviewer:'single agent manual census; unresolved is not accepted as correct',semantics,assertions,links,
  link_classes:links.reduce((a,l)=>{a[l.classification]=(a[l.classification]??0)+1;return a},{})};
fs.writeFileSync(output,JSON.stringify(report,null,2),{flag:'wx'});
console.log(JSON.stringify({semantics,link_classes:report.link_classes},null,2));
