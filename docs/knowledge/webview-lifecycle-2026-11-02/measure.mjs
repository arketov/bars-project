import { spawn, spawnSync } from 'node:child_process';
import { readFile, writeFile, mkdir, copyFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer } from 'node:net';
import { createHash } from 'node:crypto';

const directory = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(directory, '../../..');
const faultOnly = process.argv.includes('--fault-only');
const root = path.join(repo, faultOnly ? '.bars/webview-361-fault' : '.bars/webview-361');
const output = path.join(directory, faultOnly ? 'fault.json' : 'isolated.json');
const sleep = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));
const sha = data => createHash('sha256').update(data).digest('hex');
await mkdir(root, { recursive: true });
const executable = path.join(root, 'workspace_probe.exe');
await copyFile(faultOnly ? path.join(repo, '.bars/webview-361/workspace_probe.exe') : 'C:/bt/bars/debug/examples/workspace_probe.exe', executable);
await writeFile(path.join(root, 'ready.js'), 'window.__researchReady=true;');
const port = await new Promise(resolve => {
  const server = createServer();
  server.listen(0, '127.0.0.1', () => { const value = server.address().port; server.close(() => resolve(value)); });
});
const child = spawn(executable, [root, path.join(root, 'ready.js')], {
  cwd: repo, windowsHide: true, stdio: 'ignore',
  env: { ...process.env, BARS_USER_HOME: path.join(root, 'home'), BARS_PROBE_DOCUMENTS: '100', BARS_PROBE_TIMEOUT: '1800', BARS_PROBE_RETAIN: '1', WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1` },
});
const report = { success: false, root_pid: child.pid, exe_sha256: sha(await readFile(executable)), source_commit: spawnSync('git', ['rev-parse', 'HEAD'], { cwd: repo, encoding: 'utf8' }).stdout.trim(), documents: 100, profile: 'debug/FakeProvider; own loopback CDP', samples: [] };
report.source_commit_is_exact = false;
report.source_note = 'Checkout HEAD at launch; shared dirty tree. Executable and loaded JS SHA-256 identify the measured artifacts.';
let socket, nextId = 1;
const pending = new Map();
const save = () => writeFile(output, JSON.stringify(report, null, 2));
const metrics = (known = []) => {
  const result = spawnSync('pwsh', ['-NoProfile', '-File', path.join(directory, 'metrics.ps1'), '-RootId', String(child.pid), '-ExpectedPath', executable, '-KnownInstancesJson', JSON.stringify(known)], { encoding: 'utf8', windowsHide: true });
  if (result.status !== 0) throw Error(result.stderr);
  return JSON.parse(result.stdout);
};
const send = (method, params = {}) => new Promise((resolve, reject) => {
  const id = nextId++;
  const timer = setTimeout(() => { pending.delete(id); reject(Error(`CDP timeout: ${method}`)); }, 30000);
  pending.set(id, { resolve, reject, timer });
  socket.send(JSON.stringify({ id, method, params }));
});
const evaluate = async expression => {
  const response = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
  if (response.exceptionDetails) throw Error(JSON.stringify(response.exceptionDetails));
  return response.result.value;
};
const invoke = (command, args = {}) => evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)},${JSON.stringify(args)})`);
const waitFor = async (read, description, timeout = 120000) => {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) { const value = await read(); if (value) return value; await sleep(250); }
  throw Error(`Timeout: ${description}`);
};
try {
  const target = await waitFor(async () => {
    try { return (await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()).find(target => target.type === 'page' && /tauri\.localhost/.test(target.url)); } catch { return null; }
  }, 'own WebView endpoint');
  // Assert the listener belongs to this newly spawned process tree before CDP use.
  const tree = metrics();
  const listener = spawnSync('pwsh', ['-NoProfile', '-Command', `Get-NetTCPConnection -State Listen -LocalPort ${port} | Select-Object -ExpandProperty OwningProcess | ConvertTo-Json -Compress`], { encoding: 'utf8', windowsHide: true });
  const owners = [JSON.parse(listener.stdout)].flat();
  if (!owners.length || owners.some(pid => !tree.processes.some(row => row.pid === pid))) throw Error('CDP listener outside owned tree');
  socket = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
  socket.onmessage = event => {
    const value = JSON.parse(event.data), item = pending.get(value.id);
    if (!item) return;
    pending.delete(value.id); clearTimeout(item.timer);
    if (value.error) item.reject(Error(JSON.stringify(value.error))); else item.resolve(value.result);
  };
  await send('Page.enable');
  await send('Performance.enable');
  report.runtime = await send('Browser.getVersion');
  await waitFor(() => evaluate('!!window.__researchReady && !!window.__TAURI_INTERNALS__'), 'page load');
  const resources = (await send('Page.getResourceTree')).frameTree;
  report.loaded_assets = [];
  for (const resource of resources.resources.filter(value => value.type === 'Script')) {
    const content = await send('Page.getResourceContent', { frameId: resources.frame.id, url: resource.url });
    report.loaded_assets.push({ url: resource.url, sha256: sha(content.base64Encoded ? Buffer.from(content.content, 'base64') : content.content) });
  }
  const { projects } = await invoke('bars_call', { method: 'projects', params: {} });
  const project = projects[0].path;
  const call = (method, params = {}) => invoke('bars_call', { method, params: { project, cwd: project, ...params } });
  await waitFor(async () => { const value = await call('activity'); return !value.running && !value.queue.length; }, 'index idle');
  await waitFor(async () => !(await call('graph_status')).active_job, 'graph idle');
  report.corpus_hash = (await invoke('workspace_probe_metrics')).corpus_hash;
  report.fixture = { tree_documents: (await call('tree')).documents.length };
  for (const instrumented of (faultOnly ? [] : [false, true])) {
    if (instrumented) {
      const source = await readFile(path.join(directory, 'instrument.js'), 'utf8');
      report.instrument_sha256 = sha(source);
      await send('Page.addScriptToEvaluateOnNewDocument', { source });
      await send('Page.reload');
      await waitFor(() => evaluate('!!window.__researchReady && !!window.__researchStats'), 'instrumented reload');
    }
    for (const round of [1, 2, 3]) {
      for (const visible of [true, false]) {
        await invoke('workspace_probe_windows', { count: 1, visible });
        await sleep(15000);
        const before = metrics();
        const start = await evaluate(`({hidden:document.hidden,body:document.body.innerText.length,animations:document.getAnimations().map(a=>({state:a.playState,time:a.currentTime,kind:a.constructor.name})),stats:window.__researchStats?.()??null})`);
        const performanceBefore = await send('Performance.getMetrics');
        await sleep(30000);
        const performanceAfter = await send('Performance.getMetrics');
        const end = await evaluate(`({hidden:document.hidden,body:document.body.innerText.length,animations:document.getAnimations().map(a=>({state:a.playState,time:a.currentTime,kind:a.constructor.name})),stats:window.__researchStats?.()??null})`);
        const after = metrics();
        report.samples.push({ instrumented, round, visible, before, after, start, end, performanceBefore, performanceAfter });
        await save();
        process.stdout.write(`${instrumented ? 'timers' : 'baseline'} ${round} ${visible ? 'visible' : 'hidden'}\n`);
      }
    }
  }
  // Sampling profiler is separate from the CPU-counter series: it adds overhead.
  report.cpu_profiles = [];
  await send('Profiler.enable');
  await send('Profiler.setSamplingInterval', { interval: 1000 });
  for (const visible of (faultOnly ? [] : [true, false])) {
    await invoke('workspace_probe_windows', { count: 1, visible });
    await sleep(5000);
    await send('Profiler.start');
    await sleep(10000);
    const { profile } = await send('Profiler.stop');
    const file = `cpu-${visible ? 'visible' : 'hidden'}.cpuprofile`;
    await writeFile(path.join(directory, file), JSON.stringify(profile));
    const counts = new Map();
    for (const id of profile.samples ?? []) counts.set(id, (counts.get(id) ?? 0) + 1);
    report.cpu_profiles.push({ visible, file, samples: profile.samples?.length ?? 0, top: profile.nodes.map(node => ({ frame: node.callFrame, samples: counts.get(node.id) ?? 0 })).sort((a, b) => b.samples - a.samples).slice(0, 12) });
  }
  await invoke('workspace_probe_windows', { count: 1, visible: true });
  await sleep(2000);
  report.returned = await evaluate('({hidden:document.hidden,body:document.body.innerText.length,stats:window.__researchStats?.()})');
  report.success = report.returned.body > 100 && report.fixture.tree_documents === 100;
  report.fault_injection = { kind: 'CDP Browser.crash; only this isolated browser', before: metrics() };
  // Intentional failure of the owned fixture. No working-profile process is touched.
  void send('Browser.crash').then(result => { report.fault_injection.response = result; }).catch(error => { report.fault_injection.error = String(error); });
  await sleep(5000);
  report.fault_injection.after = metrics(report.fault_injection.before.processes);
  report.fault_injection.host_survived = report.fault_injection.after.native_alive;
  report.fault_injection.expected_failure_observed = report.fault_injection.before.processes.some(row => row.role === 'browser')
    && report.fault_injection.before.processes.some(row => row.role === 'renderer')
    && report.fault_injection.after.known_instances.filter(row => row.role !== 'native').every(row => !row.same_instance)
    && report.fault_injection.after.known_instances.some(row => row.role === 'native' && row.same_instance);
  report.success &&= report.fault_injection.after.known_instances.length === report.fault_injection.before.processes.length;
  report.success_means = 'Measurement completed; fault outcome and host survival are reported separately.';
  await save();
} catch (error) { report.success = false; report.error = String(error); report.host_exit_code = child.exitCode; await save(); throw error; }
finally {
  if (socket?.readyState === WebSocket.OPEN && !report.fault_injection) {
    try { await invoke('workspace_probe_finish', { result: { success: false } }); } catch { /* Собственный host завершится ниже. */ }
  }
  socket?.close();
  for (const item of pending.values()) { clearTimeout(item.timer); item.reject(Error('closed')); }
  // Only the process created by this script; retained fixture stays under .bars/.
  if (child.exitCode === null) {
    await Promise.race([new Promise(resolve => child.once('exit', resolve)), sleep(3000)]);
    if (child.exitCode === null) child.kill();
  }
  if (report.fault_injection?.before) {
    try {
      for (let attempt = 0; attempt < 10; attempt++) {
        await sleep(1000);
        report.cleanup = metrics(report.fault_injection.before.processes);
        if (report.cleanup.known_instances.every(row => !row.same_instance)) break;
      }
    } catch (error) { report.cleanup_error = String(error); }
    await save();
  }
}
