import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import KeybindingsPage from './KeybindingsPage.svelte';
import { keybindings } from './keybindings.svelte';
import type { Overrides } from './keybindings';
let component: ReturnType<typeof mount>;
beforeEach(()=>{
  keybindings.snapshot={profile:{version:1,revision:0,platforms:{}},error:null,recovery:null};keybindings.active={};keybindings.available=true;keybindings.error='';
  HTMLDialogElement.prototype.showModal=function(){this.setAttribute('open','');};HTMLDialogElement.prototype.close=function(){this.removeAttribute('open');};
});
afterEach(async()=>{await unmount(component);document.body.innerHTML='';vi.restoreAllMocks();});
const button=(root:ParentNode,text:string)=>[...root.querySelectorAll<HTMLButtonElement>('button')].find(b=>b.textContent===text)!;
const row=(title:string)=>[...document.querySelectorAll('.binding-row')].find(r=>r.querySelector('b')?.textContent===title)!;
it('конфликт блокирует запись; явная замена меняет только черновик; ошибка сохраняет правки', async()=>{
  component=mount(KeybindingsPage,{target:document.body});flushSync();
  button(row('Настройки'),'Назначить').click();flushSync();
  const dialog=document.querySelector<HTMLDialogElement>('[aria-label="Запись сочетания"]')!;
  dialog.dispatchEvent(new KeyboardEvent('keydown',{code:'KeyF',key:'А',ctrlKey:true,shiftKey:true,bubbles:true,cancelable:true}));flushSync();
  expect(button(dialog,'Принять').disabled).toBe(true);
  expect(dialog.textContent).toContain('Глобальный поиск');
  button(dialog,'Переназначить сюда').click();flushSync();
  expect(keybindings.active).toEqual({});expect(row('Глобальный поиск').textContent).toContain('Не назначено');
  const save=vi.spyOn(keybindings,'save').mockRejectedValue(new Error('Нет доступа к файлу'));
  button(document,'Сохранить сочетания').click();await Promise.resolve();await Promise.resolve();flushSync();
  expect(save).toHaveBeenCalledWith({'search.open':null,'settings.open':{code:'KeyF',modifiers:['Control','Shift']}},0,null);
  expect(document.body.textContent).toContain('Нет доступа к файлу');expect(row('Настройки').textContent).toContain('Ctrl+Shift+F');
  button(document,'Отменить / загрузить актуальные').click();flushSync();expect(row('Настройки').textContent).toContain('Ctrl+,');
});
it('сохранение включает неизвестный ID; новая ревизия не затирает открытый черновик', async()=>{
  keybindings.snapshot.profile.platforms.windows={'future.unknown':null};
  component=mount(KeybindingsPage,{target:document.body});flushSync();
  button(row('Глобальный поиск'),'Убрать').click();flushSync();
  const save=vi.spyOn(keybindings,'save').mockImplementation(async(bindings:Overrides)=>{keybindings.accept({profile:{version:1,revision:1,platforms:{windows:bindings}},error:null,recovery:null});});
  button(document,'Сохранить сочетания').click();await Promise.resolve();await Promise.resolve();flushSync();
  expect(save.mock.calls[0][0]).toEqual({'future.unknown':null,'search.open':null});
  keybindings.accept({profile:{version:1,revision:2,platforms:{windows:{}}},error:null,recovery:null});flushSync();
  expect(document.body.textContent).toContain('Карта изменена другим окном');expect(row('Глобальный поиск').textContent).toContain('Не назначено');
});
