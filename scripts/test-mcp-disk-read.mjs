// Настоящие stdio-MCP и IPC; только собственный временный профиль и фикстура.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { createHash } from 'node:crypto';

const bin = path.resolve(process.argv[2] ?? path.join(process.env.CARGO_TARGET_DIR ?? 'C:/bt/bars', 'debug'));
const evidence = path.resolve(process.argv[3] ?? `target/mcp-disk-${Date.now()}`);
assert.ok(!fs.existsSync(evidence), 'Нужна новая папка evidence');
const home = path.join(evidence, 'home');
const project = path.join(evidence, 'project');
fs.mkdirSync(home, { recursive: true });
fs.mkdirSync(path.join(project, 'docs'), { recursive: true });
fs.writeFileSync(path.join(project, 'docs/a.md'), '# А\n😀 новая строка\n[Б](b.md)\n');
fs.writeFileSync(path.join(project, 'docs/b.md'), '# Б\n');
const suffix = process.platform === 'win32' ? '.exe' : '';
const env = { ...process.env, BARS_USER_HOME: home, CODEX_HOME: path.join(home,'.codex'), BARS_TEST_FAKE_MODELS: '1' };
const children = [];
function start(exe, args = []) {
  const child = spawn(exe, args, { env, cwd: project, windowsHide: true, stdio: ['pipe','pipe','pipe'] });
  children.push(child);
  return child;
}
function exited(child) {
  if (child.exitCode !== null) return Promise.resolve(child.exitCode);
  return new Promise((resolve,reject) => { child.once('exit',resolve); child.once('error',reject); });
}
const profileFiles = ['.codex/config.toml','.codex/hooks.json','.claude.json','.claude/settings.json',
  '.agents/skills/bars-docs/SKILL.md','.agents/skills/bars-session-log/SKILL.md','.agents/skills/bars-end/SKILL.md',
  '.claude/skills/bars-docs/SKILL.md','.claude/skills/bars-session-log/SKILL.md','.claude/skills/bars-end/SKILL.md'];
function profile() {
  return Object.fromEntries(profileFiles.flatMap(name => [name,name+'.bars-backup']).map(name => {
    const file = path.join(process.env.USERPROFILE ?? process.env.HOME,name);
    return [name, fs.existsSync(file) ? [createHash('sha256').update(fs.readFileSync(file)).digest('hex'),fs.statSync(file).mtimeMs] : null];
  }));
}
const before = profile();
let worker;
let mcp;
const evidenceResult = { checks: [], profile_unchanged: false };
const deadline = setTimeout(() => { for (const child of children) if(child.exitCode === null) child.kill(); }, 90000);
try {
  worker = start(path.join(bin, 'examples/client_refresh_session'+suffix));
  let stderr = '';
  await new Promise((resolve,reject) => {
    worker.stderr.on('data', data => { stderr += data; if(stderr.includes('session: ready')) resolve(); });
    worker.once('error',reject);
    worker.once('exit', code => reject(new Error(`worker exited ${code}: ${stderr}`)));
  });
  const cli = start(path.join(bin,'bars-cli'+suffix), ['register', project]);
  let cliError = '';
  cli.stdout.resume(); cli.stderr.on('data',d => cliError += d);
  assert.equal(await exited(cli),0,cliError);
  mcp = start(path.join(bin,'bars-mcp'+suffix));
  let mcpError = '';
  mcp.stderr.on('data',d => mcpError += d);
  const pending = new Map();
  const reader = createInterface({ input:mcp.stdout });
  reader.on('line', line => { const msg = JSON.parse(line); const done = pending.get(msg.id); if(done) { pending.delete(msg.id); done(msg); } });
  mcp.once('exit', code => { for(const done of pending.values()) done({error:{message:`MCP exited ${code}`}}); pending.clear(); });
  let id = 0;
  function request(method, params) {
    return new Promise(resolve => { const next=++id; pending.set(next,resolve); mcp.stdin.write(JSON.stringify({jsonrpc:'2.0',id:next,method,params})+'\n'); });
  }
  const init = await request('initialize',{protocolVersion:'2025-11-25',capabilities:{},clientInfo:{name:'disk-proof',version:'1'}});
  assert.ok(init.result.instructions.includes('bars_text_search'));
  assert.ok([...init.result.instructions].length <= 2000);
  mcp.stdin.write(JSON.stringify({jsonrpc:'2.0',method:'notifications/initialized'})+'\n');
  const list = await request('tools/list',{});
  assert.equal(list.result.tools.length,16);
  for (const name of ['bars_text_search','bars_links']) {
    const schema = list.result.tools.find(t=>t.name===name).inputSchema;
    assert.equal(schema.additionalProperties,false);
    assert.ok(!schema.properties.window_id);
  }
  async function tool(name,args) {
    const reply = await request('tools/call',{name,arguments:{...args,format:'json'}});
    assert.ok(reply.result && !reply.result.isError,JSON.stringify(reply));
    return JSON.parse(reply.result.content.find(c=>c.type==='text').text);
  }
  const found = await tool('bars_text_search',{query:'новая строка'});
  assert.equal(found.documents[0].hits[0].column,4);
  const links = await tool('bars_links',{path:'docs/a.md'});
  assert.equal(links.items[0].target,'docs/b.md');
  fs.writeFileSync(path.join(project,'docs/a.md'),'# А\nсохранено позже\n[нет](missing.md)\n');
  assert.equal((await tool('bars_text_search',{query:'сохранено позже'})).documents[0].path,'docs/a.md');
  assert.equal((await tool('bars_links',{path:'docs/a.md'})).items[0].broken,true);
  evidenceResult.checks.push('initialize <=2000','16 schemas','stdio tools/call','UTF-16','fresh text','fresh links');
  mcp.stdin.end();
  assert.equal(await exited(mcp),0,mcpError);
  fs.writeFileSync(path.join(home,'stop'),'');
  assert.equal(await exited(worker),0,stderr);
  fs.writeFileSync(path.join(evidence,'worker.stderr.txt'),stderr);
  fs.writeFileSync(path.join(evidence,'mcp.stderr.txt'),mcpError);
} finally {
  clearTimeout(deadline);
  for(const child of children) if(child.exitCode === null) child.kill();
  evidenceResult.profile_unchanged = JSON.stringify(before) === JSON.stringify(profile());
  fs.writeFileSync(path.join(evidence,'result.json'),JSON.stringify(evidenceResult,null,2));
  assert.ok(evidenceResult.profile_unchanged,'Настоящий профиль изменился; проверьте параллельную работу');
}
console.log(JSON.stringify(evidenceResult));
