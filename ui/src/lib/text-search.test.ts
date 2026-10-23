import {beforeEach, expect, it, vi} from 'vitest';
import {call} from './api';
import {TextSearchStore, type TextOutput, type TextQuery} from './text-search.svelte';
import {app} from './app.svelte';
import {editors} from './editors.svelte';
vi.mock('./api', () => ({call:vi.fn(), inTauri:()=>false, joinPath:vi.fn(), openPath:vi.fn(), pickFolder:vi.fn()}));
const rpc=vi.mocked(call);
const params:TextQuery={project:'C:/p',query:'Привет',modules:[],files:[],case_sensitive:false,regex:false,path_filter:'',request_id:'1'};
const result=(revision='r',column=1):TextOutput=>({documents:[{path:'docs/a.md',module:'.',title:'A',revision,hits_total:10,has_more:true,hits:[{line:1,column,length:6,text:'Привет',ranges:[[0,6]]}]}],docs_total:1,hits_total:10,truncated:null,skipped:[],elapsed_ms:1});
beforeEach(()=>{vi.restoreAllMocks();rpc.mockReset();app.textSearch.reset();app.projects=[{path:'C:/p',name:'P',docs_root:'docs',state:'ready',modules:[]}];app.current='C:/p';app.query='Привет';app.tree=[];app.selected=new Set();app.retrievalMode='hybrid';});
it('текстовый ответ виден до смыслового, ошибка модели его не убирает',async()=>{
  let reject!: (e:Error)=>void;
  rpc.mockImplementation(async(method)=>{if(method==='text_search')return result();return new Promise((_,no)=>reject=no);});
  const pending=app.search(); await vi.waitFor(()=>expect(app.textSearch.output?.docs_total).toBe(1));
  expect(app.results).toBeNull();reject(Error('модель недоступна'));await pending;
  expect(app.textSearch.output?.documents[0].path).toBe('docs/a.md');expect(app.searchError?.message).toContain('модель');
});
it('новый запрос и смена проекта отклоняют старый ответ текста',async()=>{
  const store=new TextSearchStore();const done:((v:TextOutput)=>void)[]=[];
  rpc.mockImplementation(()=>new Promise(resolve=>done.push(resolve)));
  const first=store.search(params);const second=store.search({...params,query:'второй'});
  done[1](result('new'));await second;done[0](result('old'));await first;
  expect(store.output?.documents[0].revision).toBe('new');
  const third=store.search(params);store.reset();done[2](result());await third;expect(store.output).toBeNull();
});
it('порция читает только файл, при другой ревизии заменяет вхождения',async()=>{
  const store=new TextSearchStore();rpc.mockResolvedValueOnce(result());await store.search(params);
  rpc.mockResolvedValueOnce(result('r',9));await store.loadMore('docs/a.md');
  expect(rpc.mock.lastCall?.[1]).toMatchObject({path:'docs/a.md',offset:1,revision:'r'});
  expect(store.output?.documents[0].hits.map(h=>h.column)).toEqual([1,9]);
  rpc.mockResolvedValueOnce(result('changed',3));await store.loadMore('docs/a.md');
  expect(store.output?.documents[0].hits.map(h=>h.column)).toEqual([3]);
});
it('перед поиском ждёт запись, конфликт не отправляет ни один запрос',async()=>{
  let done!:(v:never[])=>void;
  vi.spyOn(editors,'unsaved','get').mockReturnValue([{}] as never);
  vi.spyOn(editors,'flush').mockImplementation(()=>new Promise(resolve=>done=resolve));
  rpc.mockRejectedValue(Error('модель'));
  const pending=app.search();expect(rpc).not.toHaveBeenCalled();done([]);await pending;
  expect(rpc.mock.calls.map(c=>c[0])).toEqual(['text_search','search']);
  rpc.mockClear();vi.mocked(editors.flush).mockResolvedValue([{path:'docs/a.md'}] as never);
  await app.search();expect(rpc).not.toHaveBeenCalled();expect(app.textSearch.error).toContain('docs/a.md');
});
it('ошибка выражения относится только к текстовой выдаче',async()=>{
  const store=new TextSearchStore();rpc.mockRejectedValue(Error('Некорректное выражение: [ в позиции 1'));
  await store.search({...params,regex:true,query:'['});expect(store.error).toContain('позиции 1');expect(store.loading).toBe(false);
});
