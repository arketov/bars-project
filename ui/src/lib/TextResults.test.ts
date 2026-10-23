import {afterEach, expect, it, vi} from 'vitest';
import {flushSync, mount, unmount} from 'svelte';
import TextResults from './TextResults.svelte';
import {app} from './app.svelte';
vi.mock('./api',()=>({call:vi.fn(),inTauri:()=>false,joinPath:vi.fn(),openPath:vi.fn(),pickFolder:vi.fn()}));
let component:ReturnType<typeof mount> | undefined;
afterEach(()=>{if(component)void unmount(component);component=undefined;app.textSearch.reset();vi.restoreAllMocks();});
it('выделяет текст безопасно и открывает точный диапазон редактора',()=>{
  app.textSearch.output={documents:[{path:'docs/a.md',title:'A',module:'.',revision:'r',hits_total:1,has_more:false,hits:[{line:8,column:4,length:6,text:'😀 <текст>',ranges:[[4,9]]}]}],docs_total:1,hits_total:1,skipped:[],truncated:null,elapsed_ms:2};
  const open=vi.spyOn(app,'openDocument').mockResolvedValue();const host=document.createElement('div');
  component=mount(TextResults,{target:host});flushSync();
  expect(host.querySelector('mark')?.textContent).toBe('текст');expect(host.querySelector('текст')).toBeNull();
  host.querySelector<HTMLButtonElement>('.occurrence')!.click();
  expect(open).toHaveBeenCalledWith({path:'docs/a.md',title:'A',first_line:8,last_line:8,column:4,length:6});
});
