// Снимки стенда 2.3.1 для согласования: node shot.mjs [регулярное выражение имён] ; стенд `npm run graph-actions --prefix ui` на :4184.
import { chromium } from "playwright-core";
import { mkdirSync } from "node:fs";
import { join } from "node:path";

const out = join(import.meta.dirname, "..", "shots");
mkdirSync(out, { recursive: true });
const browser = await chromium.launch({ executablePath: join(process.env.LOCALAPPDATA, "ms-playwright/chromium-1228/chrome-win64/chrome.exe"), args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"] });

const jobs = [];
for (const theme of ["light", "dark"])
  for (const w of [1440, 1024, 760]) {
    jobs.push({ name: `base-${theme}-${w}`, theme, w, q: { select: "MAIN_IDEA" } });
    jobs.push({ name: `path-${theme}-${w}`, theme, w, q: { select: "MAIN_IDEA", view: "path", to: "ADR-0002", sem: "mixed" } });
  }
const views = {
  local: { select: "MAIN_IDEA", view: "local", depth: "2" },
  impact: { select: "MAIN_IDEA", view: "impact" },
  context: { view: "context", ctx: "1" },
  topics: { view: "topics" },
  audit: { view: "audit" },
  entities: { level: "entities", select: "bars" },
  mixed: { level: "mixed", scenario: "conflict" },
  chunks: { level: "chunks", select: "Решение о раскладке" },
  filters: { level: "mixed", pop: "filters" },
  analysis: { pop: "analysis" },
  list: { select: "MAIN_IDEA", view: "path", to: "ADR-0002", list: "view" },
  subgraph: { select: "MAIN_IDEA", sub: "MAIN_IDEA,ADR-0001,Векторный поиск" },
  query: { query: "физический граф раскладка" },
  conditions: { level: "entities", scenario: "conditions" },
};
for (const [name, q] of Object.entries(views)) for (const theme of ["light", "dark"]) jobs.push({ name: `v-${name}-${theme}`, theme, w: 1440, q });
for (const s of ["empty", "notbuilt", "nolinks", "lagging", "fileerrors", "unresolved", "ambiguous", "limit", "stale", "deleted", "norender"]) {
  const q = { scenario: s };
  if (["lagging", "ambiguous"].includes(s)) q.level = "entities";
  if (s === "limit") Object.assign(q, { select: "MAIN_IDEA", view: "local" });
  if (s === "deleted") q.select = "Уведомления и колокольчик";
  jobs.push({ name: `s-${s}`, theme: "light", w: 1440, q });
}
jobs.push({ name: "v-reduced-light", theme: "light", w: 1440, q: { motion: "reduced", select: "MAIN_IDEA", view: "local" } });

const only = process.argv[2] ? new RegExp(process.argv[2]) : null;
for (const job of jobs.filter((j) => !only || only.test(j.name))) {
  const page = await browser.newPage({ viewport: { width: job.w, height: 900 } });
  await page.goto(`http://localhost:4184/graph-actions.html?${new URLSearchParams({ theme: job.theme, ...job.q })}`);
  await page.waitForTimeout(1200);
  await page.click('button:has-text("Граф")');
  await page.waitForTimeout(6500);
  await page.screenshot({ path: join(out, `${job.name}.jpg`), type: "jpeg", quality: 72 });
  await page.close();
  console.log(job.name);
}
await browser.close();
