import type { DockviewApi } from 'dockview-core';
import { tick } from 'svelte';
import { app } from './app.svelte';
import { catalog } from './keybindings';
import { keybindings } from './keybindings.svelte';
import { confirmClose } from './unsaved.svelte';
import { adapterFor, type Adapter } from './command-adapters';
import { documentAt, folderAt } from './contextmenu';
import { copyText, documentAction, folderAction, folderTarget } from './api';
import { panelTitles, type PanelKind } from './workspace-layout';
import type { Choice } from './CommandPalette.svelte';
type Kind = Exclude<PanelKind,'document'>;
type Target = { project:string|null; docsRoot:string; panel?:string; element:HTMLElement|null; path?:string; line?:number; folder?:boolean; adapters: Map<string,{adapter:Adapter;identity:string|number|undefined}> };
export interface WorkspaceCommands {
  api: DockviewApi;
  host: HTMLElement;
  open: (kind:Kind)=>void;
  search: ()=>void;
  reset: ()=>void;
  palette: { open:(items:Choice[],title?:string)=>void; close:(restore?:boolean)=>void; isOpen:()=>boolean };
  resize: (id:string)=>void;
}
export function workspaceCommands(options:WorkspaceCommands) {
  const { api, host, palette } = options;
  function capture():Target {
    const element=document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const panel=api.groups.find(group=>!!element && group.element.contains(element))?.activePanel ?? api.activePanel;
    const doc=documentAt(element), folder=folderAt(element);
    const adapters=new Map<string,{adapter:Adapter;identity:string|number|undefined}>();
    for(const c of catalog) {const adapter=adapterFor(c.id,element);if(adapter)adapters.set(c.id,{adapter,identity:adapter.identity?.()});}
    return {project:app.current,docsRoot:app.docsRoot,panel:panel?.id,element,path:doc?.path??folder?.path,line:doc?.line,folder:!doc&&!!folder,adapters};
  }
  function reason(id:string,target:Target):string|undefined {
    const command=catalog.find(c=>c.id===id)!;
    if (command.future) return command.future;
    if (target.project!==app.current) return 'Проект уже изменён';
    if (target.docsRoot!==app.docsRoot) return 'Папка документации уже изменилась';
    if (target.panel&&!api.getPanel(target.panel)) return 'Вкладка уже закрыта';
    const captured=target.adapters.get(id);
    if(captured) { const {adapter,identity}=captured; if(!adapter.element.isConnected||adapterFor(id,target.element)!==adapter||identity!==adapter.identity?.())return 'Цель уже изменилась'; return adapter.reason(); }
    if (['commands.open','search.open','search.main','settings.open','settings.keybindings','project.add','project.choose','panel.open','layout.reset','notifications.readAll','notifications.clear','notifications.open','search.scope','search.scopeClear','search.options'].includes(id)||id.startsWith('panel.open.')) return;
    if (id.startsWith('project.')) return app.current?undefined:'Проект не выбран';
    if (id.startsWith('target.')) return target.path&&(!target.folder||id!=='target.open')?undefined:'Выберите файл или папку';
    if (['panel.next','panel.previous','tab.next','tab.previous','tab.close','tab.move','panel.resize'].includes(id)) return target.panel?undefined:'Нет активной вкладки';
    return 'Выберите соответствующую строку или панель';
  }
  function focus(id:string) {
    const panel=api.getPanel(id); if(!panel) return;
    panel.api.setActive(); panel.group.element.scrollIntoView?.({block:'nearest',inline:'nearest'});
    const element=[...panel.group.element.querySelectorAll<HTMLElement>('.dv-content-container textarea:not(:disabled), .dv-content-container button:not(:disabled), .dv-content-container input:not(:disabled)')].find(node=>!!node.getClientRects().length) ?? panel.group.element.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]') ?? panel.group.element;
    if (element.tabIndex<0) element.tabIndex=0; element.focus();
  }
  function show(target=capture()) { palette.open(catalog.map(c=>({id:c.id,title:`${c.category} · ${c.title}`,hint:keybindings.hint(c.id),reason:reason(c.id,target),run:()=>execute(c.id,target)}))); }
  async function execute(id:string,target=capture()) {
    const unavailable=reason(id,target); if(unavailable) { app.flash(unavailable,true); return; }
    const adapter=target.adapters.get(id)?.adapter;
    if(adapter) { if(!adapter.element.isConnected) {app.flash('Цель уже удалена',true);return;} await adapter.run(); return; }
    if(id==='commands.open') {show(target);return;}
    if(id==='search.open') {options.search();return;}
    if(id==='search.main') {app.openSearch(); options.open('search'); await tick(); host.querySelector<HTMLElement>('textarea')?.focus();return;}
    if(id==='settings.open'||id==='settings.keybindings') {options.open('settings'); app.openSettings(id==='settings.keybindings'?'keybindings':'models');return;}
    if(id==='panel.open') {palette.open(Object.entries(panelTitles).map(([kind,title])=>({id:kind,title,run:()=>options.open(kind as Kind)})),'Открыть панель');return;}
    if(id.startsWith('panel.open.')) {options.open(id.slice(11) as Kind);return;}
    if(id==='layout.reset') {if(window.confirm('Сбросить раскладку? Документы и сочетания сохранятся.')) options.reset();return;}
    if(id==='project.add') {await app.addProject();return;}
    if(id==='project.choose') {await tick();document.querySelector<HTMLElement>('.workspace .proj')?.focus();return;}
    if(id==='project.remove') {app.requestDeleteProject(target.project!);return;}
    if(id==='project.checkIndex') {await app.reindexCheck(target.project!);return;}
    if(id==='notifications.readAll') {app.readNotifications();return;}
    if(id==='notifications.clear') {app.clearNotifications();return;}
    if(id==='notifications.open') {host.parentElement?.querySelector<HTMLElement>('.bell')?.focus();return;}
    if(id==='search.scopeClear') {app.clearSelection();return;}
    if(id==='search.scope'||id==='search.options') {options.search();await tick();document.querySelector<HTMLElement>(id==='search.scope'?'dialog[open] aside input':'dialog[open] select')?.focus();return;}
    if(id.startsWith('target.')) {
      if(id==='target.open') await app.openDocument({path:target.path!,title:target.path!,first_line:target.line??1,last_line:target.line??1});
      else if(target.folder) {const pinned=await folderTarget(target.project!,target.docsRoot,target.path!);const result=await folderAction(pinned.id,id==='target.reveal'?'open':'path');if(id==='target.copyPath') await copyText(result.absolute);}
      else {const result=await documentAction(target.project!,target.path!,id==='target.reveal'?'reveal':'path');if(id==='target.copyPath') await copyText(result);}
      return;
    }
    const panel=api.getPanel(target.panel!);if(!panel)return;
    if(id==='panel.next'||id==='panel.previous') {
      const groups=[...api.groups].sort((a,b)=>{const x=a.element.getBoundingClientRect(),y=b.element.getBoundingClientRect();return Math.abs(x.left-y.left)>10?x.left-y.left:x.top-y.top;});
      const next=groups[(groups.indexOf(panel.group)+(id==='panel.next'?1:-1)+groups.length)%groups.length]; if(next.activePanel)focus(next.activePanel.id);return;
    }
    if(id==='tab.next'||id==='tab.previous') {const tabs=panel.group.panels;focus(tabs[(tabs.indexOf(panel)+(id==='tab.next'?1:-1)+tabs.length)%tabs.length].id);return;}
    if(id==='tab.close') {
      const params=panel.params as {kind?:string;hit?:{path:string}}|undefined;
      // НФТ-5: вкладка с несохранёнными правками закрывается только после записи или решения пользователя.
      if(params?.kind==='document'&&params.hit&&!(await confirmClose(app.current,params.hit.path)))return;
      panel.api.close();if(api.activePanel)focus(api.activePanel.id);return;
    }
    if(id==='panel.resize') {options.resize(panel.id);return;}
    if(id==='tab.move') {
      const source=panel.group;
      const check=()=>{const error=reason(id,target);if(error||panel.group!==source)throw new Error(error??'Исходная группа уже изменилась');};
      const choices:Choice[]=api.groups.filter(g=>g!==source).map(g=>({id:g.id,title:g.panels.map(p=>p.title).join(', '),run:()=>{check();if(!api.groups.includes(g))throw new Error('Целевая группа уже закрыта');panel.api.moveTo({group:g,position:'center'});}}));
      for(const [position,title] of [['left','Слева'],['right','Справа'],['top','Сверху'],['bottom','Снизу']] as const) {
        const rect=panel.group.element.getBoundingClientRect();
        choices.push({id:position,title:`${title} от текущей группы`,reason:panel.group.panels.length<2?'В группе одна вкладка':(position==='left'||position==='right'?rect.width<500:rect.height<280)?'Недостаточно места':undefined,run:()=>{check();panel.api.moveTo({group:source,position});}});
      }
      palette.open(choices,'Переместить вкладку');
    }
  }
  let composing=false;
  const start=()=>{composing=true;},end=()=>{composing=false;};
  function route(event:KeyboardEvent) {
    if(event.defaultPrevented||composing||event.isComposing||event.repeat||keybindings.recording||app.consentOpen||app.deletingProject||document.querySelector('[data-command-blocked="true"]'))return;
    const command=keybindings.match(event);if(!command)return;
    if(palette.isOpen() && command.id==='commands.open') {event.preventDefault();palette.open([]);return;}
    const modal=[...document.querySelectorAll<HTMLDialogElement>('dialog[open]')].at(-1);
    if(modal&&(palette.isOpen()||!['search.open','commands.open'].includes(command.id)||modal.getAttribute('aria-label')!=='Глобальный поиск'))return;
    if(document.querySelector('[role="menu"]'))return;
    const target=capture();if(reason(command.id,target))return;
    event.preventDefault();event.stopImmediatePropagation();void execute(command.id,target).catch(e=>app.flash(String(e),true));
  }
  window.addEventListener('keydown',route);window.addEventListener('compositionstart',start);window.addEventListener('compositionend',end);
  return { execute,show,dispose:()=>{window.removeEventListener('keydown',route);window.removeEventListener('compositionstart',start);window.removeEventListener('compositionend',end);} };
}
