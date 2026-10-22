// Полноразмерные снимки стенда: node shot.mjs '<json>' ; json = [{name,query,w,h,theme,steps:[{js|wait|click|move|drag}]}]
import { chromium } from "playwright-core";
import { mkdirSync } from "node:fs";
import { join } from "node:path";

const out = join(import.meta.dirname, "..", "shots");
mkdirSync(out, { recursive: true });
const jobs = JSON.parse(process.argv[2]);
const browser = await chromium.launch({
  executablePath: join(process.env.LOCALAPPDATA, "ms-playwright/chromium-1228/chrome-win64/chrome.exe"),
  args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"],
});
const report = [];
for (const job of jobs) {
  const page = await browser.newPage({ viewport: { width: job.w ?? 1440, height: job.h ?? 900 }, deviceScaleFactor: job.dpr ?? 1 });
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  const q = new URLSearchParams({ theme: job.theme ?? "light", ...(job.query ?? {}) }).toString();
  await page.goto(`http://localhost:4181/graph-design.html?${q}`);
  await page.waitForTimeout(job.settle ?? 2500);
  for (const step of job.steps ?? []) {
    if (step.wait) await page.waitForTimeout(step.wait);
    if (step.click) await page.click(step.click, step.opts ?? {});
    if (step.clickAt) await page.mouse.click(step.clickAt[0], step.clickAt[1]);
    if (step.dblclickAt) await page.mouse.dblclick(step.dblclickAt[0], step.dblclickAt[1]);
    if (step.fill) await page.fill(step.fill[0], step.fill[1]);
    if (step.shot) await page.screenshot({ path: join(out, `${step.shot}.png`) });
    if (step.key) await page.keyboard.press(step.key);
    if (step.js) report.push({ name: job.name, js: await page.evaluate(step.js) });
  }
  await page.screenshot({ path: join(out, `${job.name}.png`), ...(job.clip ? { clip: { x: job.clip[0], y: job.clip[1], width: job.clip[2], height: job.clip[3] } } : {}) });
  report.push({ name: job.name, errors });
  await page.close();
}
await browser.close();
console.log(JSON.stringify(report, null, 1));
