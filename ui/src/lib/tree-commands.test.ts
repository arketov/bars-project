import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { app } from './app.svelte';
import { buildTree } from './tree';
import { executeTreeCommand, treeCommandReason, type TreeCommandTarget } from './tree-commands';

const api = vi.hoisted(() => ({folderTarget:vi.fn(),folderAction:vi.fn(),copyText:vi.fn(),documentAction:vi.fn(),call:vi.fn()}));
vi.mock('./api', () => ({...api,joinPath:(a:string,b:string)=>`${a}/${b}`,inTauri:()=>false}));
let target:TreeCommandTarget;
beforeEach(() => {
  app.current='C:/дерево'; app.docsRoot='docs'; app.workerDown=null;
  app.projects=[{path:app.current,name:'Дерево',docs_root:'docs',state:'ready',modules:[]}];
  app.tree=buildTree([{path:'docs/модуль/Тема.md',module:'модуль',title:'Тема',chunks:1}], 'docs');
  const element=document.createElement('div'); document.body.append(element);
  target={project:app.current,docsRoot:'docs',path:'модуль',element};
});
afterEach(() => {document.body.innerHTML='';app.current=null;app.projects=[];app.tree=[];vi.clearAllMocks();});
it('после смены проекта во время закрепления папки команда ОС не выполняется', async () => {
  let resolve!:(value:unknown)=>void;
  api.folderTarget.mockImplementationOnce(() => new Promise(done => {resolve=done;}));
  const result=executeTreeCommand('target.reveal',target);
  const rejection=expect(result).rejects.toThrow('Цель уже изменилась');
  app.current='C:/другой'; resolve({id:'старый',absolute:'C:/дерево/docs/модуль',missing:false});
  await rejection; expect(api.folderAction).not.toHaveBeenCalled();
});
it('удалённая строка и удалённый узел не заменяются текущим документом', () => {
  target.element.remove(); expect(treeCommandReason('target.rename',target)).toBe('Цель уже изменилась');
  document.body.append(target.element); app.tree=[];
  expect(treeCommandReason('target.rename',target)).toBe('Узел уже удалён');
});
it('неприменимые команды и недоступная проверка отвергаются перед IPC', async () => {
  await expect(executeTreeCommand('target.open',target)).rejects.toThrow('не подходит');
  app.projects=[{...app.projects[0],state:'reindex_required'}];
  await expect(executeTreeCommand('target.checkIndex',target)).rejects.toThrow('согласие');
  expect(api.folderTarget).not.toHaveBeenCalled(); expect(api.documentAction).not.toHaveBeenCalled();
});
