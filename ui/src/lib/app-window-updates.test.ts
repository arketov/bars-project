import { afterEach, expect, it, vi } from "vitest";
import type { Change, Snapshot } from "./window-updates";
const wire=vi.hoisted(()=>({snapshot:null as Snapshot|null,event:null as ((event:{payload:Change})=>void)|null}));
vi.mock("./api",async original=>({...await original<typeof import("./api")>(),inTauri:()=>true,call:vi.fn(async()=>({topics:[],total:0}))}));
vi.mock("@tauri-apps/api/core",()=>({invoke:vi.fn(async(command:string)=>command==="window_observe"?{project:"p",subscription:"t",epoch:1}:wire.snapshot)}));
vi.mock("@tauri-apps/api/event",()=>({listen:vi.fn(async(_name:string,handler:(event:{payload:Change})=>void)=>{wire.event=handler;return()=>{wire.event=null;};})}));
import { app } from "./app.svelte";
afterEach(()=>{app.stop();app.current=null;vi.restoreAllMocks();});
const disk=(root:string,path:string)=>({status:"ok" as const,docs_root:root,revision:path,dirs:[],files:[{path,size:1,modified:1}],problems:[]});
const index=(root:string,path:string)=>({docs_root:root,documents:[{path,module:".",title:path,chunks:1}]});
function snapshot(epoch:number,revision:number):Snapshot{return{project:"p",subscription:"t",epoch,revisions:[revision,revision,revision],parts:{}};}
it("an error in a new root keeps the old tree whole until the new root parts are available",async()=>{
  vi.spyOn(document,"hidden","get").mockReturnValue(false);app.current="p";
  wire.snapshot={...snapshot(1,1),parts:{disk:{revision:1,state:"ready",value:disk("docs","docs/old.md")},index:{revision:1,state:"ready",value:index("docs","docs/old.md")}}};
  await app.loadTree();const old=app.tree;expect(app.docsRoot).toBe("docs");
  wire.snapshot={...snapshot(2,2),parts:{disk:{revision:2,state:"error",error:"Новый корень недоступен",value:null},index:{revision:2,state:"ready",value:index("manual","manual/new.md")}}};
  wire.event!({payload:wire.snapshot});
  await vi.waitFor(()=>expect(app.treeProblems[0]?.reason).toContain("Новый корень"));
  expect(app.tree).toBe(old);expect(app.docsRoot).toBe("docs");
  wire.snapshot={...snapshot(2,3),parts:{disk:{revision:3,state:"ready",value:disk("manual","manual/new.md")},index:{revision:2,state:"ready",unchanged:true}}};
  wire.event!({payload:wire.snapshot});
  await vi.waitFor(()=>expect(app.docsRoot).toBe("manual"));
  expect(JSON.stringify(app.tree)).toContain("manual/new.md");expect(JSON.stringify(app.tree)).not.toContain("docs/old.md");
});
