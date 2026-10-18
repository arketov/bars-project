const $=id=>document.getElementById(id);
const escape=text=>String(text).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const baseNodes=[
 {id:'consent',label:'Согласие на переиндексацию',x:370,y:260,path:'fixtures/features/reindex-consent.md',quote:'Переиндексация выполняется после явного согласия пользователя.',role:'Поведение'},
 {id:'requirement',label:'Требование Т-25',x:140,y:135,path:'fixtures/requirements.md',quote:'Смена отпечатка embed-модели требует согласия на перестроение индекса.',role:'Требование'},
 {id:'model',label:'Отпечаток embed-модели',x:400,y:100,path:'fixtures/features/models.md',quote:'В индексе записан отпечаток embed-модели. Изменение только rerank не меняет векторы.',role:'Ограничение'},
 {id:'worker',label:'Воркер и индекс',x:610,y:210,path:'fixtures/architecture/worker.md',quote:'Воркер сохраняет предыдущий индекс до успешного завершения перестроения.',role:'Архитектура'},
 {id:'test',label:'Проверка смены модели',x:560,y:400,path:'fixtures/verification.md',quote:'Проверены отказ без согласия и сохранение прежнего индекса при отмене.',role:'Проверка'},
 {id:'decision',label:'Решение о хранении',x:300,y:450,path:'fixtures/adr/storage.md',quote:'Принято: SQLite и отдельные таблицы графа; перестройка графа не меняет embeddings.',role:'Решение'},
 {id:'settings',label:'Настройки моделей',x:120,y:340,path:'fixtures/features/settings.md',quote:'Выбор embed и rerank независим. После сохранения доступность поиска видна в состоянии проекта.',role:'Поведение'},
 {id:'history',label:'Исследование и альтернативы',x:660,y:70,path:'fixtures/knowledge/research.md',quote:'Варианты рассмотрены для исследования. Это не принятое решение проекта.',role:'Исследование'},
];
const baseLinks=[['requirement','consent','semantic'],['model','consent','semantic'],['consent','worker','link'],['test','consent','semantic'],['decision','worker','link'],['settings','consent','link'],['settings','model','link'],['history','decision','similarity'],['worker','test','link'],['model','worker','similarity']];
let nodes=baseNodes,links=baseLinks;
let selected='consent',local=false,asList=false,zoom=1,pan=[0,0],mode='relations',pinned=new Set(),stopped=true;
const stateText={partial:['Структура готова.','Явные отношения извлечены частично; отсутствие связи не доказывает отсутствие зависимости.'],not_built:['Граф ещё не построен.','Обновление строит структуру и использует сохранённые embeddings.'],building:['Построение: 8 из 12 документов.','Предыдущий согласованный снимок доступен; новое поколение ещё не опубликовано.'],cancelled:['Задание отменено.','Предыдущий согласованный снимок сохранён.'],stale:['Источники изменились.','Нужен новый снимок; раскрытие по старому курсору недоступно.'],error:['Не обработан 1 файл.','Источник недоступен. Ошибка отделена от успешной индексации остальных документов.'],empty:['В выбранной области нет объектов.','Измените область или проверьте состояние индекса.'],ambiguous:['Найдено несколько объектов «Модель».','Выберите namespace и источник; путь ещё не построен.'],conflict:['Кандидат конфликта условий.','Сравните исходные формулировки и область применимости. Автоматического вердикта нет.'],hub:['Показана ограниченная окрестность хаба.','Тестовый хаб: 250 объектов, 249 связей; в настоящем графе применяются лимиты и продолжение снимка.'],long:['Длинные подписи.','Полное имя доступно в инспекторе и представлении списком.'],renderer:['Карта недоступна.','Источники и связи доступны в списке.']};
function visible(){if(!document.querySelector('.scope input').checked)return [];if(!local)return nodes;let ids=new Set([selected]),front=new Set([selected]);for(let i=0;i<Number($('depth').value);i++){const next=new Set();for(const[a,b]of links){if(front.has(a))next.add(b);if(front.has(b))next.add(a);}front=next;for(const id of front)ids.add(id);}return nodes.filter(n=>ids.has(n.id));}
function label(n){if($('scenario').value==='long'&&n.id==='consent')return 'Правила согласия при изменении модели и параметров индексации в нескольких независимых проектах';return n.label;}
function sourceButton(n){return `<button class="link" data-source="${n.id}">Открыть источник · строки 8–12 ↗</button>`;}
function card(n){return `<article class="card"><small>${escape(n.role)} · явный источник</small><h3>${escape(label(n))}</h3><p class="mono">${escape(n.path)}</p><blockquote>${escape(n.quote)}</blockquote>${sourceButton(n)}</article>`;}
function render(){
 const s=$('scenario').value,[title,text]=stateText[s];
 const view=$('view').value;
 const kind=view==='entities'?'entity':'document';
 nodes=baseNodes.map(n=>({...n,kind}));links=[...baseLinks];
 if(view==='sections'||view==='chunks'||view==='mixed'){
   const extras=baseNodes.flatMap((n,i)=>{
    const kinds=view==='mixed'?['section','chunk','entity']:[view==='sections'?'section':'chunk'];
    return kinds.map((kind,j)=>({...n,id:n.id+':'+kind,kind,label:(kind==='section'?'Раздел: ':kind==='chunk'?'Чанк: ':'Понятие: ')+n.label,x:n.x+20*(j+1),y:n.y+55*(j+1)}));
   });nodes.push(...extras);for(const n of extras)links.push([n.id.split(':')[0],n.id,'link']);
 }
 if(s==='hub'){
  nodes=Array.from({length:250},(_,i)=>({...baseNodes[i%8],id:i===0?'consent':'hub-'+i,kind:'document',label:i===0?'Хаб':'Документ '+i,x:380+Math.cos(i*2.4)*Math.sqrt(i)*20,y:280+Math.sin(i*2.4)*Math.sqrt(i)*16}));
  links=nodes.slice(1).map(n=>['consent',n.id,'link']);
 }
 if(!nodes.some(n=>n.id===selected))selected=nodes[0].id;$('notice').innerHTML=`<b>${title}</b> ${text}`;
 const shown=visible(),ids=new Set(shown.map(n=>n.id)),edges=links.filter(([a,b])=>ids.has(a)&&ids.has(b));
 $('counts').textContent=`${shown.length} объектов · ${edges.length} связей`;
 const empty=shown.length===0||['not_built','empty','stale'].includes(s);$('empty').hidden=!empty;$('empty').querySelector('h2').textContent=shown.length?title:'В области нет объектов';$('empty').querySelector('p').textContent=text;
 $('canvas').toggleAttribute('hidden',empty||asList||s==='renderer');$('nodeList').hidden=empty||(!asList&&s!=='renderer');
 $('rebuild').textContent=s==='building'?'Остановить':'Обновить граф';
 const glyph=n=>({entity:'◇',chunk:'▤',section:'§',document:'▣'}[n.kind]);
 $('scene').innerHTML=edges.map(([a,b,k])=>{const x=nodes.find(n=>n.id===a),y=nodes.find(n=>n.id===b);return `<line x1="${x.x}" y1="${x.y}" x2="${y.x}" y2="${y.y}" class="${k}"/>`;}).join('')+shown.map(n=>`<g class="node ${n.id===selected?'selected':''}" data-node="${n.id}" tabindex="0" role="button" aria-label="${escape(label(n))}" transform="translate(${n.x} ${n.y})"><circle r="21"/><text class="glyph" y="5">${glyph(n)}</text><text y="44">${escape(label(n).length>30?label(n).slice(0,29)+'…':label(n))}</text>${pinned.has(n.id)?'<text x="22" y="-20">⌖</text>':''}</g>`).join('');
 $('scene').setAttribute('transform',`translate(${pan[0]} ${pan[1]}) scale(${zoom})`);
 $('nodeList').innerHTML=shown.map(n=>`<button data-node="${n.id}" class="${n.id===selected?'active':''}">${glyph(n)} ${escape(label(n))} · ${escape(n.role)}</button>`).join('');
 inspector();
}
function inspector(){
 const n=nodes.find(n=>n.id===selected)||nodes[0];$('title').textContent=label(n);$('kind').textContent=({entity:'СУЩНОСТЬ',section:'РАЗДЕЛ',chunk:'ЧАНК',document:'ДОКУМЕНТ'}[n.kind])+' · ФИКСТУРА';$('meta').innerHTML=`${escape(n.path)}<br>Снимок fixture-p1 · источник: исходный текст${pinned.has(n.id)?' · позиция закреплена':''}`;
 const operation=mode;
 if(operation==='paths'){
  const target=nodes.find(x=>x.id!==selected)?.id;
  $('detail').innerHTML=`<label>Второй конец пути <select id="target">${nodes.filter(x=>x.id!==selected).map(x=>`<option value="${x.id}">${escape(x.label)}</option>`).join('')}</select></label><div id="pathDetail"></div>`;
  const show=()=>{
   const to=$('target').value,queue=[[selected]],seen=new Set([selected]);let path=null;
   while(queue.length){const p=queue.shift(),id=p.at(-1);if(id===to){path=p;break;}for(const[a,b]of links){const next=a===id?b:b===id?a:null;if(next&&!seen.has(next)){seen.add(next);queue.push([...p,next]);}}}
   $('pathDetail').innerHTML=path?path.slice(1).map((id,i)=>{const edge=links.find(([a,b])=>a===path[i]&&b===id||b===path[i]&&a===id),node=nodes.find(n=>n.id===id);return `<div class="path-step"><b>${i+1} · ${escape(nodes.find(n=>n.id===path[i]).label)} → ${escape(node.label)}</b><p>Тип перехода: ${edge[2]} · ${edge[0]===path[i]?'по направлению':'обратный обход'}</p>${sourceButton(node)}</div>`;}).join('')+'<p>Полная цепочка фикстуры · '+(path.length-1)+' переходов</p>':'<p>Путь в тестовом графе не найден.</p>';
  };$('target').value=target;$('target').onchange=show;show();
 }
 else if(operation==='impact')$('detail').innerHTML=`<h3>Что пересмотреть</h3><p>Документированное влияние, 2 пути</p>${card(nodes[3])}${card(nodes[4])}<small>Ссылки-соседи показываются отдельно от зависимостей.</small>`;
 else if(operation==='context')$('detail').innerHTML=`<h3>Контекст задачи</h3><p>Бюджет: 3 180 из 4 096 оценочных токенов</p>${[nodes[1],nodes[5],nodes[3],nodes[4]].map(card).join('')}<div class="card"><b>Не найдена роль</b><p>Ограничения для macOS. Неполнота обработки не доказывает отсутствие документа.</p></div>`;
 else if(operation==='explore')$('detail').innerHTML=`<h3>Темы выбранной области</h3><div class="card"><h3>Модели и совместимость</h3><p>3 документа · 2 представителя</p><button class="link" data-node="model">Раскрыть группу ↗</button></div><div class="card"><h3>Индексация и хранение</h3><p>3 документа · 2 представителя</p><button class="link" data-node="worker">Раскрыть группу ↗</button></div><small>Название — по источникам. Группы не являются авторской классификацией.</small>`;
 else if(operation==='audit')$('detail').innerHTML=`<h3>Аудит документации</h3><div class="card"><h3>Структурный дефект</h3><p>1 неразрешённая ссылка</p>${sourceButton(nodes[6])}</div><div class="card"><h3>Кандидат пробела</h3><p>Для объекта не найдена явная связь verifies. Смысловой слой обработан частично.</p></div><small>Изоляты и смысловые пробелы считаются отдельно.</small>`;
 else $('detail').innerHTML=`<div class="row"><span class="pill">Явное правило</span><span class="pill">requires</span><span class="pill warn">Условие: смена embed</span></div>${card(n)}<button class="ghost" id="pin">${pinned.has(n.id)?'Освободить позицию':'Закрепить позицию'}</button>`;
 if($('scenario').value==='ambiguous')$('detail').innerHTML=`<h3>Выберите объект «Модель»</h3>${[nodes[2],nodes[6]].map(n=>`<button class="card" data-node="${n.id}">${escape(n.label)}<br><small>${escape(n.path)}</small></button>`).join('')}<p>Отношения не выбираются автоматически.</p>`;
 if($('scenario').value==='conflict')$('detail').innerHTML=`<h3>Кандидат конфликта</h3><div class="card"><span class="pill">Положительное</span><p>GPU требуется · условие: ускоритель выбран</p>${sourceButton(nodes[2])}</div><div class="card"><span class="pill">Отрицательное</span><p>GPU не требуется · условие: CPU</p>${sourceButton(nodes[6])}</div><small>Разные условия могут объяснять различие. Это не установленный дефект.</small>`;
}
document.addEventListener('click',e=>{
 const node=e.target.closest('[data-node]');if(node){selected=node.dataset.node;render();return;}
 const op=e.target.closest('[data-op]');if(op){mode=op.dataset.op;inspector();return;}
 const source=e.target.closest('[data-source]');if(source){const n=nodes.find(n=>n.id===source.dataset.source);$('sourcePath').textContent=n.path+' · строки 8–12 · fixture-p1';$('sourceQuote').textContent=n.quote;$('source').showModal();return;}
 if(e.target.id==='pin'){pinned.has(selected)?pinned.delete(selected):pinned.add(selected);render();}
});
$('scene').addEventListener('keydown',e=>{if(['Enter',' '].includes(e.key)){e.preventDefault();const id=e.target.closest('[data-node]')?.dataset.node;if(id){selected=id;render();document.querySelector('[data-node="'+CSS.escape(id)+'"]')?.focus();}}});
$('scenario').onchange=render;$('view').onchange=render;$('group').onchange=()=>{mode=$('group').value==='community'?'explore':'relations';render();};$('depth').onchange=render;
$('local').onclick=()=>{local=!local;$('local').setAttribute('aria-pressed',String(local));render();};$('list').onclick=()=>{asList=!asList;$('list').setAttribute('aria-pressed',String(asList));render();};
$('plus').onclick=()=>{zoom=Math.min(2.5,zoom+.15);render();};$('minus').onclick=()=>{zoom=Math.max(.5,zoom-.15);render();};$('fit').onclick=()=>{zoom=1;pan=[0,0];render();};
let layoutFrame=0,layoutStep=0;
$('stop').textContent='Запустить раскладку';
$('stop').onclick=()=>{stopped=!stopped;cancelAnimationFrame(layoutFrame);$('stop').textContent=stopped?'Продолжить раскладку':'Остановить раскладку';
 if(!stopped){layoutStep=0;const step=()=>{if(stopped)return;layoutStep++;for(const n of baseNodes)if(!pinned.has(n.id)){n.x+=Math.sin(layoutStep/8)*.8;n.y+=Math.cos(layoutStep/8)*.8;}render();if(layoutStep<90)layoutFrame=requestAnimationFrame(step);else{stopped=true;$('stop').textContent='Запустить раскладку';}};layoutFrame=requestAnimationFrame(step);}};
$('theme').onclick=()=>{const dark=document.documentElement.dataset.mode!=='dark';document.documentElement.dataset.mode=dark?'dark':'light';$('theme').textContent=dark?'Светлая тема':'Тёмная тема';};
$('narrow').onclick=()=>{$('app').classList.toggle('narrow');};$('sourceClose').onclick=()=>$('source').close();$('close').onclick=()=>{$('detail').innerHTML='<p>Выберите объект карты или списка.</p>';$('title').textContent='Объект не выбран';};
$('rebuild').onclick=()=>{const active=$('scenario').value==='building';$('scenario').value=active?'cancelled':'building';$('rebuild').textContent=active?'Обновить граф':'Остановить';render();};
$('search').onclick=()=>{mode=$('retrieval').value==='graph'?'paths':'context';inspector();$('extra').innerHTML=$('retrieval').value==='mix'?'<div class="card"><h3>Связанные источники</h3><p>Отдельный пакет · полный путь · собственный бюджет</p><button class="link" data-node="worker">Показать путь в графе ↗</button></div>':'';};
$('searchTab').onclick=()=>{$('heading').textContent='Поиск со связанными источниками';$('searchTab').classList.add('selected');$('graphTab').classList.remove('selected');$('search').click();};$('graphTab').onclick=()=>{$('heading').textContent='Граф документации';$('graphTab').classList.add('selected');$('searchTab').classList.remove('selected');};
let drag=null;$('canvas').addEventListener('pointerdown',e=>{if(e.target.closest('[data-node]'))return;drag=[e.clientX,e.clientY,...pan];$('canvas').setPointerCapture(e.pointerId);});$('canvas').addEventListener('pointermove',e=>{if(drag){pan=[drag[2]+e.clientX-drag[0],drag[3]+e.clientY-drag[1]];$('scene').setAttribute('transform',`translate(${pan[0]} ${pan[1]}) scale(${zoom})`);}});$('canvas').addEventListener('pointerup',()=>{drag=null;});
render();
document.querySelector('.scope input').onchange=render;
