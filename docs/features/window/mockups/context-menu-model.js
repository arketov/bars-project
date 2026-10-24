/* Макет шага 3.3.3: модель контекстных меню окна (document-actions.md, «Модель меню окна»). Меню собирается из таблицы пунктов, как в модели. Данные вымышленные, действий ОС нет. */
(() => {
  const {esc,icon}=Mock;
  const SCENES=[
    ['file-tree','Файл в дереве'],['file-tree-unavailable','Файл: проект недоступен'],['file-starting','Файл: воркер запускается'],
    ['folder2','Папка второго уровня'],['folder3','Папка третьего уровня'],['rootgroup','Группа «Корневые документы»'],['project','Проект'],
    ['tab','Вкладка документа'],['header','Шапка документа'],['card','Карточка выдачи'],['editor','Редактор: правка текста'],['editor-sub','Редактор: подменю «Документ»'],
    ['render-sel','Рендер с выделенным текстом'],['link','Ссылка в тексте'],['broken','Битая ссылка'],['heading','Заголовок в структуре'],['panel-tab','Вкладка служебной панели'],['topic','Строка «Новых тем»'],['edge','Меню у правого нижнего края'],
    ['del-links','Удаление файла: есть входящие ссылки'],['del-none','Удаление файла: ссылок нет'],['del-folder','Удаление папки со ссылками'],['del-unchecked','Удаление: ссылки не проверены']
  ];
  const NOTES={
    'file-tree':'Набор документа плюс «Цвет…» между «путём» и «проверкой». «Удалить…» последним, отдельной группой.',
    'file-tree-unavailable':'«Проверить индекс» подходит файлу, но сейчас недоступен: серый, причина в подсказке и внизу меню. Серый пункт остаётся в обходе стрелками.',
    'file-starting':'Пока воркер запускается, серыми становятся «Переименовать…», «Переместить…» и «Удалить…».',
    'folder2':'Создать · изменить · путь · «Цвет и иконка…» · «Проверить индекс проекта» · удаление. «Открыть» у папки нет.',
    'folder3':'То же без «Цвет и иконка…».',
    'rootgroup':'Нет «Переименовать», «Переместить», «Удалить» — группа не папка.',
    'project':'Только проверка и удаление проекта; удаление последним.',
    'tab':'Группа вкладки первой; «Открыть» скрыт — документ уже открыт. «Закрыть справа» серое: справа вкладок нет. Сочетания из реестра — Ctrl+W, Ctrl+Shift+M; у остальных назначений нет, текста нет.',
    'header':'Тот же набор, что у вкладки: шапка документа — объект «файл», место «шапка».',
    'card':'Набор документа без группы вкладки; «Открыть» есть, потому что документ ещё не открыт.',
    'editor':'Сверху правка текста, ниже одно подменю «Документ ▸» вместо прежней россыпи пунктов.',
    'editor-sub':'Подменю несёт группы вкладки того же документа в том же порядке. Стрелка влево или Escape закрывает его, второй Escape — меню.',
    'render-sel':'Выделенный текст добавляет сверху группу правки с «Копировать»; ниже меню рендера.',
    'link':'Цель — документ ссылки, а не открытый документ. Нет файловых пунктов.',
    'broken':'Битая ссылка: «Создать документ» и путь.',
    'heading':'Перейти к заголовку и скопировать ссылку на заголовок.',
    'panel-tab':'У служебной панели цели-файла нет: только действия вкладки.',
    'topic':'«Закрыть тему» — опасное, последнее.',
    'edge':'Меню прижимается к краю окна и не выходит за него.',
    'del-links':'Диалог при открытии считает входящие ссылки. Число названо, список раскрывается. Фокус на «Отмена».',
    'del-none':'Без ссылок диалог говорит «Входящих ссылок нет».',
    'del-folder':'Для папки добавлено число документов внутри; ссылки внутри папки не считаются.',
    'del-unchecked':'Индекс ссылок недоступен: «Ссылки не проверены» вместо числа; удаление остаётся возможным, потому что идёт в корзину.'
  };
  const scene=Mock.init({name:'menu-model',scenes:SCENES});
  const p=document.querySelector('#prototype');
  const reveal=/Mac/.test(navigator.platform)?'Показать в Finder':'Показать в Проводнике';
  let ctx=null,menu=null,submenu=null,returnTo=null,anchor={x:20,y:100},opening=false;

  /* ---------- Модель: группы в одном порядке ---------- */
  const GROUPS=['edit','tab','open','create','modify','path','style','index','delete'];
  const it=(id,label,group,o={})=>({id,label,group,...o});

  const sc=s=>s;
  const docItems=place=>{
    const out=[];
    if(place==='tab'||place==='header'||place==='render'){
      out.push(it('tab.close','Закрыть','tab',{sc:'Ctrl+W'}),it('tab.closeOthers','Закрыть другие','tab'),
        it('tab.closeRight','Закрыть справа','tab',{off:'Справа нет вкладок в группе'}),it('tab.move','Переместить вкладку…','tab',{sc:'Ctrl+Shift+M'}));
    }
    if(place==='tree'||place==='card')out.push(it('document.open','Открыть','open'));
    out.push(it('document.newWindow','Открыть в новом окне','open'),
      it('open.with','Открыть в …','open',{children:[it('ed.default','Программа по умолчанию','open'),it('ed.code','VS Code','open'),it('ed.notepad','Блокнот','open')]}),
      it('target.rename','Переименовать…','modify',{busy:true}),it('target.move','Переместить…','modify',{busy:true}),
      it('target.reveal',reveal,'path'),it('target.copyPath','Скопировать путь','path'));
    if(place==='tree')out.push(it('target.style','Цвет…','style'));
    out.push(it('target.checkIndex','Проверить индекс','index',{needs:true}),it('target.delete','Удалить…','delete',{danger:true,busy:true}));
    return out;
  };
  const editItems=[it('e.undo','Отменить','edit',{sc:'Ctrl+Z'}),it('e.redo','Повторить','edit',{sc:'Ctrl+Y'}),it('e.cut','Вырезать','edit',{sc:'Ctrl+X'}),it('e.copy','Копировать','edit',{sc:'Ctrl+C'}),it('e.paste','Вставить','edit',{sc:'Ctrl+V'}),it('e.all','Выделить всё','edit',{sc:'Ctrl+A'})];
  const dirItems=level=>[
    it('file.create','Новый документ…','create'),it('folder.create','Новая папка…','create'),
    it('target.rename','Переименовать…','modify',{busy:true}),it('target.move','Переместить…','modify',{busy:true}),
    it('target.reveal',reveal,'path'),it('target.copyPath','Скопировать путь','path'),
    ...(level===2?[it('target.style','Цвет и иконка…','style')]:[]),
    it('project.checkIndex','Проверить индекс проекта','index',{needs:true}),it('target.delete','Удалить…','delete',{danger:true,busy:true})];

  /* Цель → список пунктов. Чистая функция: рисует результат ContextMenu, пунктов сам не добавляет. */
  function itemsFor(c){
    switch(c.kind){
      case 'file': {
        const base=docItems(c.place);
        return c.sel?[it('e.copy','Копировать','edit',{sc:'Ctrl+C'}),...base]:base;
      }
      case 'editor': return [...editItems,it('doc.sub','Документ','path',{children:docItems('tab').map(x=>({...x}))})].map(x=>x.id==='doc.sub'?{...x,group:'open'}:x);
      case 'folder': return dirItems(c.level);
      case 'root': return [it('file.create','Новый документ…','create'),it('folder.create','Новая папка…','create'),it('target.reveal',reveal,'path'),it('target.copyPath','Скопировать путь','path'),it('project.checkIndex','Проверить индекс проекта','index',{needs:true})];
      case 'project': return [it('project.checkIndex','Проверить индекс','index',{needs:true}),it('project.delete','Удалить проект…','delete',{danger:true})];
      case 'panel-tab': return [it('tab.close','Закрыть','tab',{sc:'Ctrl+W'}),it('tab.closeOthers','Закрыть другие','tab'),it('tab.closeRight','Закрыть справа','tab'),it('tab.move','Переместить вкладку…','tab',{sc:'Ctrl+Shift+M'})];
      case 'link': return [it('link.open','Открыть цель','open'),it('document.newWindow','Открыть в новом окне','open'),it('link.copy','Скопировать ссылку','path')];
      case 'broken': return [it('link.createTarget','Создать документ','create'),it('link.copy','Скопировать ссылку','path')];
      case 'heading': return [it('heading.goTo','Перейти к заголовку','open'),it('heading.copyLink','Скопировать ссылку на заголовок','path')];
      case 'topic': return [it('topic.copy','Скопировать запрос','path'),it('topic.close','Закрыть тему','delete',{danger:true})];
    }
    return [];
  }
  /* Недоступность: подходит объекту, но сейчас нельзя. Причина всегда есть. */
  function reasonOf(item,c){
    if(item.needs&&c.state==='unavailable')return 'Папка проекта недоступна. Сначала восстановите доступ.';
    if(item.busy&&c.state==='starting')return 'Воркер запускается. Подождите несколько секунд.';
    if(item.off)return item.off;
    if(item.id==='tab.closeRight'&&c.kind==='panel-tab')return '';
    return '';
  }
  /* Группы в одном порядке, пустая не показывается, разделитель только между показанными. */
  function layout(items,c){
    const out=[];
    for(const g of GROUPS){
      const part=items.filter(x=>x.group===g).map(x=>({...x,reason:reasonOf(x,c)}));
      if(!part.length)continue;
      if(out.length)out.push({sep:true});
      out.push(...part);
    }
    return out;
  }

  /* ---------- Рабочая область макета ---------- */
  document.querySelector('#sidebar .project-box').dataset.menuKind='project';
  document.querySelector('#sidebar .project-box').tabIndex=0;
  const tree=[
    ['folder2','Features',0,'folder','folder',2],['folder3','window','deep','folder','folder',3],['file-tree','projects.md','deep','file','file',0],['rootgroup','Корневые документы','', 'folder','root',0]
  ];
  document.querySelector('#tree').innerHTML=tree.map(([id,name,cls,ic,kind,level])=>`<div class="tree-item ${cls}" data-menu-kind="${kind}" data-menu-place="tree" data-level="${level}" data-scene="${id}" tabindex="0">${icon(ic)}<span class="nm">${esc(name)}</span></div>`).join('')+`<p class="muted small" style="margin:14px 8px">Правый клик или Shift+F10 на любом объекте.</p>`;
  document.querySelector('#content').innerHTML=`<div class="tabs" role="tablist">
      <button class="tab current menu-target" role="tab" data-menu-kind="file" data-menu-place="tab" data-scene="tab">${icon('file')}projects.md<span class="x">×</span></button>
      <button class="tab menu-target" role="tab" data-menu-kind="panel-tab" data-scene="panel-tab">${icon('search')}Поиск<span class="x">×</span></button></div>
    <header class="doc-head menu-target" data-menu-kind="file" data-menu-place="header" data-scene="header" tabindex="0"><div><b>Проекты bars</b><small>docs/features/projects.md</small></div><span class="badge">Открыт</span></header>
    <div class="note" id="note"></div>
    <div class="doc-body" data-menu-kind="file" data-menu-place="render" data-scene="render-sel">
      <h2 data-menu-kind="heading" data-scene="heading" tabindex="0">Регистрация проекта</h2>
      <p id="sel-text">Проект регистрируется один раз; документ связан с <a href="#" data-menu-kind="link" data-scene="link">описанием индексации</a> и с <a href="#" class="broken" data-menu-kind="broken" data-scene="broken">документом, которого ещё нет</a>.</p>
      <div class="src menu-target" data-menu-kind="editor" data-scene="editor" tabindex="0"># Регистрация проекта\n\nПроект регистрируется один раз. См. [[индексация]].</div>
      <div class="card menu-target" data-menu-kind="file" data-menu-place="card" data-scene="card" tabindex="0"><b>Индексация документов</b><p>docs/features/indexing/Index.md · найдено по запросу «очередь»</p></div>
      <div class="topic-row menu-target" data-menu-kind="topic" data-scene="topic" tabindex="0"><span>«как строится граф» · 3 запроса</span><span class="muted">новая тема</span></div>
    </div>
    <dialog id="del" aria-labelledby="del-title"></dialog>`;
  /* Вызов диалога дальше создаёт содержимое заново. */
  const del=document.querySelector('#del');

  /* ---------- Меню ---------- */
  function close(focus=true){menu?.remove();submenu?.remove();menu=submenu=null;document.querySelectorAll('.context-target').forEach(n=>n.classList.remove('context-target'));if(focus)returnTo?.focus?.();}
  function fit(el,x,y){const r=p.getBoundingClientRect();el.style.left=Math.max(8,Math.min(x,r.width-el.offsetWidth-8))+'px';el.style.top=Math.max(8,Math.min(y,r.height-el.offsetHeight-8))+'px';}
  function render(entries,label,extra=''){
    const m=document.createElement('div');m.className='menu';m.setAttribute('role','menu');m.setAttribute('aria-label',label);
    m.innerHTML=entries.map((e,i)=>e.sep?'<div class="sep" role="separator"></div>':`<button class="item${e.danger?' danger':''}" role="menuitem" data-i="${i}" ${e.reason?`aria-disabled="true" aria-description="${esc(e.reason)}" title="${esc(e.reason)}" data-reason="${esc(e.reason)}"`:''} ${e.children?'aria-haspopup="menu" aria-expanded="false"':''}><span>${esc(e.label)}</span>${e.children?'<span class="sc">›</span>':e.sc?`<span class="sc">${esc(e.sc)}</span>`:''}</button>`).join('')+'<div class="reason" hidden></div>';
    m._entries=entries;p.append(m);return m;
  }
  function hint(m,btn){const r=m.querySelector('.reason');const t=btn?.dataset.reason;r.hidden=!t;r.textContent=t?'Недоступно: '+t:'';}
  function wire(m){
    m.querySelectorAll('.item').forEach(b=>{
      b.onclick=()=>pick(m,m._entries[b.dataset.i],b);
      b.onfocus=b.onmouseenter=()=>hint(m,b);
      b.onmouseleave=()=>hint(m,null);
      if(m._entries[b.dataset.i].children)b.addEventListener('mouseenter',()=>openSub(m,b));
    });
    m.onkeydown=e=>{
      const f=[...m.querySelectorAll('.item')],i=f.indexOf(document.activeElement);
      if(['ArrowDown','ArrowUp','Home','End'].includes(e.key)){e.preventDefault();f[e.key==='Home'?0:e.key==='End'?f.length-1:(i+(e.key==='ArrowDown'?1:-1)+f.length)%f.length].focus();}
      if((e.key==='ArrowRight')&&document.activeElement.getAttribute('aria-haspopup')){e.preventDefault();openSub(m,document.activeElement);}
      if(e.key==='ArrowLeft'&&m===submenu){e.preventDefault();closeSub();}
      if(e.key==='Tab'){e.preventDefault();close();}
    };
  }
  function openSub(m,btn){
    closeSub();const e=m._entries[btn.dataset.i];
    submenu=render(layout(e.children,ctx),e.label);submenu.classList.add('submenu');
    const x=parseFloat(m.style.left),y=btn.getBoundingClientRect().top-p.getBoundingClientRect().top;
    submenu.style.left='0px';submenu.style.top='0px';
    const right=x+m.offsetWidth+4,fitsRight=right+submenu.offsetWidth<p.clientWidth-8;
    fit(submenu,fitsRight?right:x-submenu.offsetWidth-4,y-4);wire(submenu);btn.setAttribute('aria-expanded','true');
    if(document.activeElement===btn||m.contains(document.activeElement))submenu.querySelector('.item')?.focus();
  }
  function closeSub(){if(!submenu)return;submenu.remove();submenu=null;menu?.querySelectorAll('[aria-expanded=true]').forEach(b=>{b.setAttribute('aria-expanded','false');});}
  function pick(m,e,b){
    if(!e||e.reason)return;                 /* Enter на сером ничего не делает */
    if(e.children){openSub(m,b);return;}
    if(e.id==='target.delete'||e.id==='project.delete'){const c=ctx;close(false);openDelete(c.dialog||'del-links');return;}
    close();Mock.toast(`Команда «${e.label}» (${e.id}). В макете она ничего не выполняет.`);
  }
  function openMenu(c,el,x,y,focusFirst=true){
    close(false);ctx=c;returnTo=el;anchor={x,y};opening=true;requestAnimationFrame(()=>opening=false);
    el.closest('.tree-item,.menu-target')?.classList.add('context-target');
    const entries=layout(itemsFor(c),c);
    const sel=c.sel?'':'';
    menu=render(entries,'Действия объекта');fit(menu,x,y);wire(menu);
    if(focusFirst)(menu.querySelector('.item:not([aria-disabled=true])')||menu.querySelector('.item'))?.focus({preventScroll:true});
    return menu;
  }
  function ctxOf(el,scn){
    const kind=el.dataset.menuKind,place=el.dataset.menuPlace;
    const c={kind,place,state:'',dialog:'del-links'};
    if(kind==='folder')c.level=Number(el.dataset.level)||2;
    if(scn==='file-tree-unavailable')c.state='unavailable';
    if(scn==='file-starting')c.state='starting';
    if(scn==='render-sel')c.sel=true;
    if(scn==='folder2'||scn==='folder3')c.dialog='del-folder';
    return c;
  }
  function targetFor(scn){
    const map={'file-tree':'[data-scene="file-tree"]','file-tree-unavailable':'[data-scene="file-tree"]','file-starting':'[data-scene="file-tree"]','edge':'[data-scene="card"]'};
    return document.querySelector(map[scn]||`[data-scene="${scn==='editor-sub'?'editor':scn}"]`);
  }
  /* Клик и клавиатура на любом размеченном объекте: цель — ближайший размеченный предок. */
  p.addEventListener('contextmenu',e=>{
    if(e.target.closest('.menu,dialog'))return;
    const el=e.target.closest('[data-menu-kind]');if(!el)return;
    e.preventDefault();const r=p.getBoundingClientRect(),scn=el.dataset.scene;
    const c=ctxOf(el,scn);c.sel=!!getSelection()?.toString()&&el.dataset.menuPlace==='render';
    openMenu(c,el,e.clientX-r.left,e.clientY-r.top);
  });
  p.addEventListener('keydown',e=>{
    if(menu||!(e.key==='ContextMenu'||(e.key==='F10'&&e.shiftKey)))return;
    const el=e.target.closest?.('[data-menu-kind]');if(!el)return;
    e.preventDefault();const r=p.getBoundingClientRect(),b=el.getBoundingClientRect();
    openMenu(ctxOf(el,el.dataset.scene),el,b.left-r.left+24,b.bottom-r.top-4);
  });
  document.addEventListener('keydown',e=>{if(e.key==='Escape'&&menu){e.preventDefault();if(submenu){const b=menu.querySelector('[aria-haspopup]');closeSub();b?.focus();}else close();}});
  document.addEventListener('pointerdown',e=>{if(menu&&!e.target.closest('.menu')&&!e.target.closest('.demo-bar'))close(false);});
  document.addEventListener('scroll',e=>{if(menu&&!opening&&!e.target.closest?.('.menu'))close(false);},true);
  window.addEventListener('blur',()=>close(false));
  new ResizeObserver(()=>{if(menu){fit(menu,anchor.x,anchor.y);closeSub();}}).observe(p);

  /* ---------- Диалог удаления в корзину ---------- */
  const LINKS=[['features/indexing/queue.md','Очередь документов',['12: «см. проект»'],'[проект](../window/projects.md)'],['features/search/hybrid.md','Гибридный поиск',['40: «карточка проекта»','77: «проекты bars»'],'']];
  function openDelete(kind){
    const folder=kind==='del-folder',none=kind==='del-none',unchecked=kind==='del-unchecked';
    const name=folder?'window':'projects.md';
    const links=folder?[...LINKS,['manual/projects.md','Проекты: руководство',['5: «зарегистрировать»','18: «удаление»'],'']]:LINKS;
    const n=links.reduce((s,l)=>s+l[2].length,0);
    const head=`Удалить в корзину: ${esc(name)}?`;
    const inside=folder?`<p class="small">Документов внутри: 12.</p>`:'';
    let refs;
    if(unchecked)refs=`<p class="banner" style="margin:10px 0">${icon('alert')}<span>Ссылки не проверены: индекс ссылок сейчас недоступен. Удаление возможно, файл уйдёт в корзину.</span></p>`;
    else if(none)refs=`<p class="small">Входящих ссылок нет.</p>`;
    else refs=`<p class="small"><b>Входящих ссылок: ${n}, в документах: ${links.length}.</b> Они станут битыми.</p><details><summary>Показать ссылки</summary><ul class="dlg-list">${links.map(l=>`<li><b>${esc(l[1])}</b><small>${esc(l[0])}</small>${l[2].map(x=>`<small>строка ${esc(x)}</small>`).join('')}</li>`).join('')}</ul></details>`;
    del.innerHTML=`<h2 id="del-title" style="font-size:16px">${head}</h2>${inside}${refs}<p class="muted small">Файл попадёт в корзину ОС, его можно восстановить оттуда.</p><div class="dlg-actions"><button class="ghost" id="del-cancel" autofocus>Отмена</button><button class="btn-danger" id="del-ok">Удалить в корзину</button></div>`;
    del.querySelector('#del-cancel').onclick=()=>{del.close();returnTo?.focus?.();};
    del.querySelector('#del-ok').onclick=()=>{del.close();Mock.toast('Узел отправлен в корзину. В макете ничего не удаляется.');};
    del.showModal();del.querySelector('#del-cancel').focus();
  }

  /* ---------- Сцены ---------- */
  function load(){
    close(false);if(del.open)del.close();
    const s=scene.value;document.querySelector('#note').textContent=NOTES[s]||'';
    if(s.startsWith('del-')){returnTo=document.querySelector('[data-scene="file-tree"]');openDelete(s);return;}
    const el=targetFor(s),r=p.getBoundingClientRect(),b=el.getBoundingClientRect(),c=ctxOf(el,s);
    if(s==='edge'){c.kind='file';c.place='card';openMenu(c,el,p.clientWidth-10,p.clientHeight-10);return;}
    const x=el.closest('aside')?Math.min(b.right-r.left+8,260):Math.min(b.left-r.left+60,p.clientWidth-260);
    const y=el.closest('aside')?b.top-r.top:b.bottom-r.top-6;
    const m=openMenu(c,el,x,y,s!=='editor-sub');
    if(s==='editor-sub'){const btn=m.querySelector('[aria-haspopup]');btn.focus();openSub(m,btn);}
  }
  scene.onchange=load;load();
})();
