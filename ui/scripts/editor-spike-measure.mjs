// Замер редактора CodeMirror 6 (этап 3.1.2): поднимает стенд, ставит puppeteer-core во временную папку,
// гоняет measure/fullParse в headless Edge и пишет JSON. Запуск: node scripts/editor-spike-measure.mjs <out.json>
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createServer } from "vite";

const out = process.argv[2];
if (!out) throw new Error("укажите файл результата");
const edge = process.env.BARS_EDGE ?? "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe";
const temp = mkdtempSync(join(tmpdir(), "bars-editor-spike-"));
let server;
try {
  execFileSync("npm", ["init", "-y"], { cwd: temp, shell: true, stdio: "ignore" });
  execFileSync("npm", ["i", "puppeteer-core@latest"], { cwd: temp, shell: true, stdio: "ignore" });
  const puppeteer = createRequire(join(temp, "x.js"))("puppeteer-core");
  server = await createServer({ configFile: "vite.editor.config.ts", logLevel: "error" });
  await server.listen();
  const browser = await puppeteer.launch({ executablePath: edge, headless: "new", args: ["--enable-precise-memory-info"] });
  const page = await browser.newPage();
  await page.setViewport({ width: 900, height: 700 });
  await page.goto("http://localhost:4182/editor.html", { waitUntil: "load" });
  const version = await browser.version();
  const measure = await page.evaluate(async () => {
    const rows = [];
    for (const kb of [10, 100, 1000, 5000])
      for (const lang of ["plain", "gfm", "gfm+code"]) {
        if (kb === 5000 && lang === "gfm+code") continue;
        rows.push(await window.spike.measure(kb * 1024, { lang, wrap: true }));
      }
    return rows;
  });
  const parse = await page.evaluate(() => [100, 1000, 5000].map((kb) => window.spike.fullParse(kb * 1024)));
  await browser.close();
  writeFileSync(out, JSON.stringify({ browser: version, date: new Date().toISOString(), measure, parse }, null, 1));
} finally {
  await server?.close();
  rmSync(temp, { recursive: true, force: true });
}
