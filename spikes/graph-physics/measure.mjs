import { snapshot } from './export.mjs';
import { build } from 'vite';
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { gzipSync, gunzipSync } from 'node:zlib';
import { resolve, join, dirname } from 'node:path';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
const here = dirname(fileURLToPath(import.meta.url));
const out = resolve(process.argv[2]);
assert(!existsSync(out), 'Папка доказательств должна быть новой');
mkdirSync(out, { recursive: true });
const requireRuntime = createRequire(resolve(process.argv[3], 'package.json'));
const { chromium } = requireRuntime('playwright');
const captured = process.argv[5]
  ? JSON.parse(gunzipSync(readFileSync(resolve(process.argv[5]))))
  : [snapshot(join(here, '../..'), 'bars-project'), snapshot(join(here, '../../../cyberProject/ai-mutiplayer-bots'), 'overdrive-bots')];
writeFileSync(join(out, 'snapshots.json.gz'), gzipSync(JSON.stringify(captured)), { flag: 'wx' });
writeFileSync(join(out, 'manifests.json'), JSON.stringify(captured.map(c => c.manifest), null, 2));
await build({ root: here, configFile: false, logLevel: 'warn', build: { outDir: join(here, 'dist'), lib: { entry: join(here, 'browser.mjs'), formats: ['es'], fileName: 'browser' } } });
const server = createServer((req, res) => {
  if (req.url === '/') { res.setHeader('Content-Type', 'text/html'); res.end('<!doctype html><meta charset="utf-8"><style>body{margin:0;background:#202127;color:#ddd;font:12px sans-serif}h1{font-size:14px}#graph{overflow:hidden}</style><h1>Исследовательский стенд — физика графа, не макет продукта</h1><div id="graph"></div><script type="module" src="/browser.js"></script>'); return; }
  const file = resolve(here, 'dist', '.' + new URL(req.url, 'http://localhost').pathname);
  if (!file.startsWith(join(here, 'dist') + '\\') && !file.startsWith(join(here, 'dist') + '/')) { res.writeHead(403).end(); return; }
  try { res.setHeader('Content-Type', 'text/javascript'); res.end(readFileSync(file)); } catch { res.writeHead(404).end(); }
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
let browser;
const report = { measuredAt: new Date().toISOString(), method: 'headless Chromium; 120 synchronous iterations (not equal convergence); three ordered repeats; 90 rAF pan frames, common d3 positions, max 30 labels; full indexed graph projections; no product/native acceptance', host: { node: process.version, platform: process.platform }, physics: [], render: [], checks: {}, packages: {} };
function save() { writeFileSync(join(out, 'measurement.json'), JSON.stringify(report, null, 2)); }
try {
  browser = await chromium.launch({ headless: true, ...(process.argv[4] ? { executablePath: resolve(process.argv[4]) } : {}) });
  const page = await browser.newPage({ viewport: { width: 1200, height: 850 }, deviceScaleFactor: 1 });
  const errors = []; page.on('pageerror', e => errors.push(String(e)));
  await page.goto(`http://127.0.0.1:${server.address().port}`);
  await page.waitForFunction(() => typeof window.physics === 'function');
  report.host.userAgent = await page.evaluate(() => navigator.userAgent);
  report.host.webgl = await page.evaluate(() => { const c = document.createElement('canvas'), gl = c.getContext('webgl2'); const ext = gl?.getExtension('WEBGL_debug_renderer_info'); return ext ? gl.getParameter(ext.UNMASKED_RENDERER_WEBGL) : 'unavailable'; });
  report.checks.weights = await page.evaluate(() => window.weightCheck());
  // Do not assert an assumed monotonic final distance for a global physical layout.
  assert(report.checks.weights.d3OneTick[1] < report.checks.weights.d3OneTick[0], 'Более сильная пружина должна сильнее притянуть за один tick');
  for (const c of captured) for (const g of c.views) {
    let common;
    for (let repeat = 0; repeat < 3; repeat++) for (const method of ['cose', 'd3', 'fa2']) {
      const result = await page.evaluate(({ g, method }) => window.physics(g, method), { g, method });
      assert(result.finite, 'Неконечные координаты');
      if (method === 'd3' && repeat === 0) common = result.positions;
      const { positions, ...metrics } = result;
      report.physics.push({ graph: g.name, nodes: g.nodes.length, edges: g.edges.length, method, repeat, ...metrics }); save();
    }
    const positioned = { ...g, nodes: g.nodes.map((n, i) => ({ ...n, x: common[i][0], y: common[i][1] })) };
    for (const [width, height] of [[320, 260], [1000, 650]]) for (let repeat = 0; repeat < 3; repeat++) for (const method of ['cytoscape', 'canvas', 'sigma']) {
      const result = await page.evaluate(({ g, method, width, height }) => window.render(g, method, width, height), { g: positioned, method, width, height });
      assert.equal(result.remainingCanvas, 0);
      report.render.push({ graph: g.name, nodes: g.nodes.length, edges: g.edges.length, method, width, height, repeat, ...result }); save();
    }
    report.checks[g.name] = await page.evaluate(g => window.workerCheck(g), g);
    assert.equal(report.checks[g.name].late, 0);
    assert(report.checks[g.name].heartbeat > 2);
    console.log(JSON.stringify({ complete: g.name, nodes: g.nodes.length, edges: g.edges.length }));
  }
  await page.screenshot({ path: join(out, 'stand.png') });
  assert.deepEqual(errors, []); report.checks.pageErrors = errors;
  const npmCli = join(dirname(process.execPath), 'node_modules/npm/bin/npm-cli.js');
  for (const name of Object.keys(JSON.parse(readFileSync(join(here, 'package.json'))).dependencies)) {
    const pkg = JSON.parse(readFileSync(join(here, 'node_modules', name, 'package.json')));
    report.packages[name] = JSON.parse(execFileSync(process.execPath, [npmCli, 'view', `${name}@${pkg.version}`, 'version', 'license', 'time', 'repository', '--json'], { encoding: 'utf8' }));
  }
  report.success = true; save();
} finally { if (browser) await browser.close(); await new Promise(resolve => server.close(resolve)); save(); }
