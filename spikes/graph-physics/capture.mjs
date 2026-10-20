import { build } from 'vite';
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { readFileSync, writeFileSync } from 'node:fs';
import { gunzipSync } from 'node:zlib';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
const here = dirname(fileURLToPath(import.meta.url)), out = resolve(process.argv[2]);
const { chromium } = createRequire(resolve(process.argv[3], 'package.json'))('playwright');
const captured = JSON.parse(gunzipSync(readFileSync(join(out, 'snapshots.json.gz'))));
await build({ root: here, configFile: false, logLevel: 'warn', build: { outDir: join(here, 'dist-capture'), lib: { entry: join(here, 'browser.mjs'), formats: ['es'], fileName: 'browser' } } });
const server = createServer((req, res) => {
  if (req.url === '/') { res.setHeader('Content-Type','text/html'); res.end('<!doctype html><meta charset="utf-8"><style>body{margin:0;background:#202127;color:#ddd;font:12px sans-serif}h1{font-size:14px}</style><h1></h1><div id="graph"></div><script type="module" src="/browser.js"></script>'); return; }
  const file = resolve(here, 'dist-capture', '.' + new URL(req.url,'http://localhost').pathname);
  if (!file.startsWith(join(here, 'dist-capture') + '\\') && !file.startsWith(join(here, 'dist-capture') + '/')) { res.writeHead(403).end(); return; }
  try { res.setHeader('Content-Type','text/javascript'); res.end(readFileSync(file)); } catch { res.writeHead(404).end(); }
});
await new Promise(r => server.listen(0,'127.0.0.1',r));
const browser = await chromium.launch({ headless: true, executablePath: resolve(process.argv[4]) });
const evidence = [];
try {
  for (const c of captured) for (const method of ['cose','d3','fa2']) {
    const page = await browser.newPage({ viewport: { width: 1000, height: 700 } });
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    await page.waitForFunction(() => typeof window.physics === 'function');
    const g = c.views[0];
    if (method === 'd3') {
      const check = await page.evaluate(g=>window.pinStopCheck(g), g);
      if (!check.pinned || !check.stopped || !check.released) throw Error(JSON.stringify(check));
      writeFileSync(join(out,`${c.manifest.name}-pin-stop.json`),JSON.stringify(check,null,2));
    }
    const positions = await page.evaluate(({g,method}) => window.physics(g,method).positions, {g,method});
    await page.evaluate(({g,positions,title}) => window.capture({...g,nodes:g.nodes.map((n,i)=>({...n,x:positions[i][0],y:positions[i][1]}))},title), {g,positions,title:`${g.name}: ${method}, 120 итераций`});
    await page.evaluate(() => new Promise(r => requestAnimationFrame(()=>requestAnimationFrame(r))));
    const file = `${c.manifest.name}-${method}.png`;
    await page.screenshot({ path: join(out,file) }); evidence.push(file); await page.close();
  }
  const tuned = [];
  for (const c of captured) for (const g of c.views) {
    const page = await browser.newPage({ viewport: {width:1000,height:700} });
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    await page.waitForFunction(()=>typeof window.tunedD3 === 'function');
    const result = await page.evaluate(g=>window.tunedD3(g),g);
    if (!result.finite) throw Error('tuned d3 coordinates');
    tuned.push({graph:g.name,elapsedMs:result.elapsedMs,finite:result.finite});
    if (g.name.endsWith('/documents')) {
      await page.evaluate(({g,positions})=>window.capture({...g,nodes:g.nodes.map((n,i)=>({...n,x:positions[i][0],y:positions[i][1]}))},g.name+': d3, отдельная настройка'),{g,positions:result.positions});
      await page.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));
      const file = `${c.manifest.name}-d3-tuned.png`; await page.screenshot({path:join(out,file)}); evidence.push(file);
    }
    await page.close();
  }
  writeFileSync(join(out,'tuning.json'),JSON.stringify({method:'One additional d3 setting, 120 ticks; exploratory, not compared as original benchmark or product defaults',results:tuned},null,2));
  writeFileSync(join(out,'captures.json'),JSON.stringify(evidence,null,2));
} finally { await browser.close(); await new Promise(r=>server.close(r)); }
