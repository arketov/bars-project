// Снимки стенда графа раунда 27 (порт 4183) в настоящем Edge через playwright-core: полный граф, режимы, выбор, приближение, мини-граф.
import { chromium } from "playwright-core";
import { mkdirSync } from "node:fs";

const out = process.argv[2];
mkdirSync(out, { recursive: true });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const base = "http://localhost:4183/graph-stand.html";

async function open(query, { width = 1600, height = 1000, wait = 22000 } = {}) {
  const page = await browser.newPage({ viewport: { width, height } });
  await page.goto(`${base}?${query}`);
  await page.waitForTimeout(wait);
  return page;
}
const full = (page) => page.locator(".pg:not(.mini) canvas");
const mini = (page) => page.locator(".pg.mini");

async function shot(page, locator, name) {
  await locator.screenshot({ path: `${out}/${name}.png` });
  console.log(name);
}

for (const [data, theme] of [["overdrive", "light"], ["bars", "light"], ["overdrive", "dark"]]) {
  const page = await open(`data=${data}&view=graph&theme=${theme}`);
  const stage = page.locator(".pg:not(.mini) .stage");
  await shot(page, stage, `${data}-${theme}-links`);
  if (theme === "light") {
    // Выбор хаба с клавиатуры: стрелка выбирает узел с наибольшим числом связей.
    await full(page).focus();
    await page.keyboard.press("ArrowRight");
    await page.waitForTimeout(600);
    await shot(page, stage, `${data}-${theme}-links-selected`);
    await page.keyboard.press("Escape");
    // Приближение к центру: подписи и стрелки.
    const box = await full(page).boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    for (let i = 0; i < 9; i++) await page.mouse.wheel(0, -120);
    await page.waitForTimeout(800);
    await page.mouse.move(box.x + 5, box.y + 5);
    await page.waitForTimeout(300);
    await shot(page, stage, `${data}-${theme}-links-zoom`);
    // Режим «по смыслу».
    await page.getByRole("radio", { name: /По смыслу/ }).first().click().catch(() => page.keyboard.press("m"));
    await page.waitForTimeout(20000);
    await full(page).focus();
    await page.keyboard.press("f");
    await page.waitForTimeout(800);
    await shot(page, stage, `${data}-${theme}-meaning`);
  }
  await page.close();
}

// Мини-граф: локальный граф открытого документа.
for (const [data, focus] of [["overdrive", "docs/PLAN.md"], ["bars", "features/graphrag/physical-graph.md"], ["bars", "docs/MAIN_IDEA.md"]]) {
  const page = await open(`data=${data}&focus=${encodeURIComponent(focus)}`, { wait: 14000 });
  await shot(page, mini(page), `mini-${data}-${focus.split("/").pop().replace(".md", "")}`);
  await page.close();
}
// Мини-граф без открытого документа — общий вид проекта.
{
  const page = await open("data=overdrive", { wait: 20000 });
  await shot(page, mini(page), "mini-overdrive-project");
  await shot(page, page.locator("body"), "window-overdrive");
  await page.close();
}
await browser.close();
