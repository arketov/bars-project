import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import Workspace from './Workspace.svelte';
import ContextMenu from './ContextMenu.svelte';
import { app } from './app.svelte';
import { buildTree } from './tree';
import { entries } from './entries.svelte';
import { keybindings } from './keybindings.svelte';
import { call } from './api';

vi.mock('./api', async original => ({...await original<typeof import('./api')>(), call:vi.fn()}));
vi.mock('./graph-renderer', () => ({GraphRenderer:class {set() {} destroy() {}}}));
const mounted: ReturnType<typeof mount>[] = [];
const settle = async () => { await new Promise(resolve => setTimeout(resolve, 0)); flushSync(); };
const send = (code:string) => window.dispatchEvent(new KeyboardEvent('keydown',{key:code,code,bubbles:true,cancelable:true}));
beforeEach(async () => {
  localStorage.clear(); keybindings.active = {}; keybindings.recording = false;
  vi.mocked(call).mockImplementation(() => new Promise(() => {}));
  app.current='C:/дерево'; app.docsRoot='docs'; app.projects=[{path:app.current,name:'Дерево',docs_root:'docs',state:'ready',modules:[]}];
  app.tree=buildTree([{path:'docs/Тема.md',module:'.',title:'Тема',chunks:1}], 'docs');
  app.workerDown=null; app.viewer=null; app.consentOpen=false; app.deletingProject=null; app.view='main';
  mounted.push(mount(Workspace,{target:document.body}),mount(ContextMenu,{target:document.body})); flushSync(); await settle();
  document.querySelector<HTMLButtonElement>('.tree .caret')!.click(); flushSync();
});
afterEach(async () => {
  entries.cancel(); for (const component of mounted.splice(0)) await unmount(component);
  document.body.innerHTML=''; keybindings.active={}; app.current=null; app.projects=[]; app.tree=[]; app.workerDown=null;
  vi.clearAllMocks();
});
it('снятый F2 не перехватывается; новое назначение открывает переименование выбранного файла', async () => {
  const row=document.querySelector<HTMLElement>('[data-menu-kind="file"]')!; row.focus();
  keybindings.active={'target.rename':null}; send('F2'); await settle(); expect(entries.request).toBeNull();
  keybindings.active={'target.rename':{code:'F3',modifiers:[]}};
  send('F2'); await settle(); expect(entries.request).toBeNull();
  send('F3'); await settle(); expect(entries.request).toMatchObject({action:'rename',node:{path:'./Тема.md'}});
});
it('Delete требует узел дерева, открывает подтверждение; в текстовом поле не удаляет файл', async () => {
  const row=document.querySelector<HTMLElement>('[data-menu-kind="file"]')!; row.focus();
  send('Delete'); await settle(); expect(entries.request?.action).toBe('trash');
  entries.cancel(); flushSync();
  const field=document.createElement('input'); document.body.append(field); field.focus();
  send('Delete'); await settle(); expect(entries.request).toBeNull();
});
it('назначенная клавиша выбора программы раскрывает то же подменю', async () => {
  const row=document.querySelector<HTMLElement>('[data-menu-kind="file"]')!; row.focus();
  keybindings.active={'target.openExternal':{code:'F4',modifiers:[]}};
  send('F4'); await settle(); await settle();
  expect(document.querySelector('.sub [role="menuitem"]')?.textContent).toContain('Программа по умолчанию');
  const parent=document.querySelector<HTMLElement>('[aria-haspopup="menu"]')!;
  expect(parent.textContent).toContain('F4');
  const oldDialog=document.createElement('dialog'); document.body.append(oldDialog);
  oldDialog.dispatchEvent(new Event('close')); await settle();
  expect(document.querySelector('.sub')).not.toBeNull();
  send('Escape'); await settle(); expect(document.querySelector('.sub')).toBeNull(); expect(document.activeElement).toBe(parent);
});
