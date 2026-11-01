import { afterEach, expect, it, vi } from "vitest";
import { ProjectUpdates, type Change, type Snapshot, type Transport } from "./window-updates";

let store: ProjectUpdates;
afterEach(() => {store?.stop(); vi.useRealTimers(); vi.restoreAllMocks();});
function deferred<T>() { let resolve!: (value:T)=>void; const promise=new Promise<T>(r=>resolve=r); return {promise,resolve}; }
function response(project="a", revision=1, token=project, epoch=1): Snapshot {
  return {project,subscription:token,epoch,revisions:[revision,revision,revision],parts:{index:{revision,state:"ready",value:{docs_root:"docs",documents:[]}}}};
}
function fixture() {
  vi.spyOn(document,"hidden","get").mockReturnValue(false);
  let event: (change:Change)=>void=()=>{};
  const api: Transport={listen:vi.fn(async handler=>{event=handler;return vi.fn();}),observe:vi.fn(async project=>project?{project,subscription:project,epoch:1}:null),snapshot:vi.fn(async token=>response(token))};
  const applied=vi.fn(), failed=vi.fn(); store=new ProjectUpdates(applied,failed,api);
  return {api,applied,failed,event:(change:Change)=>event(change)};
}

it("an in-flight lease completes before binding the next project",async()=>{
  const f=fixture();await store.bind("a",false);
  const pending=deferred<void>(), action=vi.fn(()=>pending.promise);
  const lease=store.lease(action);await vi.waitFor(()=>expect(action).toHaveBeenCalledTimes(1));
  const binding=store.bind("b",false);await Promise.resolve();
  expect(f.api.observe).not.toHaveBeenCalledWith("b",expect.anything());
  pending.resolve();await Promise.all([lease,binding]);
  expect(f.api.observe).toHaveBeenLastCalledWith("b",["disk","index","status"]);
});

it("a queued old lease cannot revoke a newer binding",async()=>{
  const f=fixture();await store.bind("a",false);
  const action=vi.fn(async()=>{}), lease=store.lease(action), binding=store.bind("b",false);
  await Promise.all([lease,binding]);
  expect(action).not.toHaveBeenCalled();
  expect(f.api.observe).toHaveBeenLastCalledWith("b",["disk","index","status"]);
});

it("listens before observe and catches an event before the initial snapshot answer",async()=>{
  const f=fixture(); const initial=deferred<Snapshot>();
  vi.mocked(f.api.snapshot).mockReturnValueOnce(initial.promise).mockResolvedValueOnce(response("a",2));
  const binding=store.bind("a",false);
  await vi.waitFor(()=>expect(f.api.snapshot).toHaveBeenCalledTimes(1));
  expect(vi.mocked(f.api.listen).mock.invocationCallOrder[0]).toBeLessThan(vi.mocked(f.api.observe).mock.invocationCallOrder[0]);
  f.event({...response("a",2)});
  initial.resolve(response("a",1)); await binding;
  expect(f.applied).toHaveBeenLastCalledWith(expect.objectContaining({revisions:[2,2,2]}));
  expect(f.api.snapshot).toHaveBeenCalledTimes(2);
});

it("ignores an old project's response and subscribes document windows without status",async()=>{
  const f=fixture(), old=deferred<Snapshot>();
  vi.mocked(f.api.snapshot).mockReturnValueOnce(old.promise).mockResolvedValueOnce(response("b",1));
  const first=store.bind("a",false);await vi.waitFor(()=>expect(f.api.snapshot).toHaveBeenCalledTimes(1));
  await store.bind("b",true); old.resolve(response("a",99)); await first;
  expect(f.applied.mock.calls.every(([snapshot])=>snapshot.project==="b")).toBe(true);
  expect(f.api.observe).toHaveBeenCalledWith("b",["disk","index"]);
});

it("an explicit refresh during IO waits for a follow-up force",async()=>{
  const f=fixture(); await store.bind("a",false);
  const slow=deferred<Snapshot>();vi.mocked(f.api.snapshot).mockReturnValueOnce(slow.promise).mockResolvedValueOnce(response("a",3));
  const first=store.refresh(false), explicit=store.refresh(true);
  let finished=false;void explicit.then(()=>{finished=true;});
  await Promise.resolve();expect(finished).toBe(false);
  slow.resolve(response("a",2));await Promise.all([first,explicit]);
  expect(f.api.snapshot).toHaveBeenLastCalledWith("a",expect.anything(),true);
  expect(f.applied).toHaveBeenLastCalledWith(expect.objectContaining({revisions:[3,3,3]}));
});

it("a new epoch supersedes an in-flight old epoch and stops fetching while hidden",async()=>{
  const f=fixture();await store.bind("a",false);
  const slow=deferred<Snapshot>();vi.mocked(f.api.snapshot).mockReturnValueOnce(slow.promise).mockResolvedValueOnce(response("a",4,"a",2));
  const reading=store.refresh(false);f.event(response("a",4,"a",2));slow.resolve(response("a",3));await reading;
  expect(f.applied).not.toHaveBeenCalledWith(expect.objectContaining({epoch:1,revisions:[3,3,3]}));
  expect(f.applied).toHaveBeenLastCalledWith(expect.objectContaining({epoch:2}));
  vi.spyOn(document,"hidden","get").mockReturnValue(true);const before=vi.mocked(f.api.snapshot).mock.calls.length;
  store.stop();await Promise.resolve();await store.refresh(true);
  expect(f.api.snapshot).toHaveBeenCalledTimes(before);
});

it("a transient failure preserves the last success and retries a full subscription",async()=>{
  const f=fixture();await store.bind("a",false);vi.useFakeTimers();
  vi.mocked(f.api.snapshot).mockRejectedValueOnce(Error("Временно недоступен"));
  await store.refresh(false);expect(f.failed).toHaveBeenCalledWith("Временно недоступен");
  expect(f.applied).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(1000);
  expect(f.api.observe).toHaveBeenCalledTimes(2);
  expect(f.applied).toHaveBeenCalledTimes(2);
});
