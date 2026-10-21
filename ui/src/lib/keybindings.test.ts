import { expect, it } from 'vitest';
import { catalog, bindingFor, conflicts, fromEvent, invalid, label, overlaps, runtimeOverrides, signature, validate } from './keybindings';

it('каталог уникален; умолчания допустимы на обеих платформах', () => {
  expect(new Set(catalog.map(c => c.id)).size).toBe(catalog.length);
  for (const os of ['windows','macos'] as const) expect(validate({},os)).toEqual([]);
});
it('null снимает сочетание, отсутствие оставляет умолчание; физическая RU/EN позиция', () => {
  const command=catalog.find(c=>c.id==='search.open')!;
  expect(bindingFor(command,{'search.open':null},'windows')).toBeNull();
  const binding=bindingFor(command,{},'windows');
  for(const key of ['F','А']) expect(signature(fromEvent(new KeyboardEvent('keydown',{key,code:'KeyF',ctrlKey:true,shiftKey:true})))).toBe(signature(binding));
  expect(label(binding)).toBe('Ctrl+Shift+F (А)');
  expect(fromEvent(new KeyboardEvent('keydown',{key:'Dead',code:'KeyF',ctrlKey:true}))).toBeNull();
  expect(fromEvent(new KeyboardEvent('keydown',{key:'А',ctrlKey:true}))).toBeNull();
  expect(fromEvent(new KeyboardEvent('keydown',{key:'Process',code:'KeyF',ctrlKey:true}))).toBeNull();
  expect(fromEvent(new KeyboardEvent('keydown',{code:'KeyF',ctrlKey:true,isComposing:true}))).toBeNull();
  const altgr=new KeyboardEvent('keydown',{code:'KeyF',ctrlKey:true,altKey:true});
  Object.defineProperty(altgr,'getModifierState',{value:(key:string)=>key==='AltGraph'});expect(fromEvent(altgr)).toBeNull();
});

it('сохранённая команда имеет приоритет над новым конфликтным умолчанием',()=>{
  const override={'search.open':{code:'Comma',modifiers:['Control']}};
  const resolved=runtimeOverrides(override,'windows');
  expect(resolved.active).toEqual({...override,'settings.open':null});expect(resolved.errors).toHaveLength(1);
  const bad=runtimeOverrides({...override,'settings.open':override['search.open']},'windows');expect(bad.active).toEqual({});
});
it('пересечения контекстов и защищённые комбинации', () => {
  expect(overlaps('editor','graph')).toBe(false); expect(overlaps('window','editor')).toBe(true);
  const binding={code:'KeyF',modifiers:['Control','Shift']};
  expect(conflicts({'settings.open':binding},'windows').map(pair=>pair.map(c=>c.id))).toContainEqual(['search.open','settings.open']);
  for(const code of ['KeyA','KeyC','KeyX','KeyV','KeyZ','KeyY','KeyR','F5','Escape']) expect(invalid({code,modifiers:['Control']},'windows')).toBeTruthy();
  expect(invalid({code:'KeyQ',modifiers:['Meta']},'macos')).toBeTruthy();
  expect(invalid({code:'KeyF',modifiers:[]},'windows')).toBeTruthy();
  expect(invalid({code:'KeyF',modifiers:['Alt']},'windows')).toBeTruthy();
  expect(invalid({code:'KeyF',modifiers:['Meta']},'windows')).toBeTruthy();
});
