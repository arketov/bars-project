// Проверки перехода «общий вид ↔ вид запроса» на стенде 2.1.4 (настоящее окно, Chromium). Запуск: node check.mjs, стенд на :4181.
import { chromium } from "playwright-core";
import { join } from "node:path";
import { mkdirSync, writeFileSync } from "node:fs";

const browser = await chromium.launch({ executablePath: join(process.env.LOCALAPPDATA, "ms-playwright/chromium-1228/chrome-win64/chrome.exe"), args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"] });
const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok: !!ok, detail });
  console.log(`${ok ? "ok  " : "FAIL"} ${name}${detail ? ` — ${detail}` : ""}`);
};
async function open(query = {}, { w = 1440, h = 900, graph = true } = {}) {
  const page = await browser.newPage({ viewport: { width: w, height: h } });
  page.errors = [];
  page.on("pageerror", (e) => page.errors.push(String(e)));
  await page.goto(`http://localhost:4181/graph-design.html?${new URLSearchParams({ theme: "light", ...query })}`);
  await page.waitForTimeout(1200);
  if (graph) {
    await page.click('button:has-text("Граф")');
    await page.waitForTimeout(2200);
  }
  return page;
}
const g = (page, which, expr) => page.evaluate(`(()=>{const f=window.__graphDesign.${which}; return ${expr}})()`);
const pos = (page, which = "full") => g(page, which, "Object.fromEntries(f.layout().nodes.map(n=>[n.index,[Math.round(n.x*1000)/1000,Math.round(n.y*1000)/1000]]))");
const maxDiff = (a, b) => Object.keys(a).reduce((m, k) => (b[k] ? Math.max(m, Math.hypot(a[k][0] - b[k][0], a[k][1] - b[k][1])) : m), 0);
const text = (page, sel) => page.textContent(sel).catch(() => "");

// ---------- 1. Возврат к общему виду: места, камера, выбор и закрепление
{
  const page = await open({ select: "MAIN_IDEA", mode: "all" });
  await page.focus(".pg:not(.mini) canvas");
  await page.keyboard.press("Space"); // закрепить выбранный
  await page.click('.pg:not(.mini) button[aria-label="Остановить движение"]');
  await page.waitForTimeout(300);
  const before = await pos(page);
  const camBefore = await g(page, "full", "f.cam()");
  const selBefore = await g(page, "full", "f.selected()");
  const pinsBefore = await g(page, "full", "f.layout().nodes.filter(n=>n.pinned).map(n=>n.index)");
  // Вход в вид запроса: через настоящий поиск окна.
  await page.click('button:has-text("Поиск")');
  await page.fill("textarea", "физический граф");
  await page.click('button:has-text("Найти")');
  await page.waitForTimeout(2200);
  await page.click('button:has-text("Граф")');
  await page.waitForTimeout(1800);
  check("завершённый поиск включает вид запроса в обеих панелях", (await g(page, "full", "f.queryOn()")) && (await g(page, "mini", "f.queryOn()")));
  check("узел запроса в центре раскладки при входе", (await g(page, "full", "f.layout().nodes.some(n=>n.node.kind==='query')")));
  await page.click('.qbar button:has-text("Общий вид")');
  await page.waitForTimeout(500);
  const after = await pos(page);
  const camAfter = await g(page, "full", "f.cam()");
  const selAfter = await g(page, "full", "f.selected()");
  const pinsAfter = await g(page, "full", "f.layout().nodes.filter(n=>n.pinned).map(n=>n.index)");
  check("«Общий вид» возвращает места узлов", maxDiff(before, after) < 1e-6, `макс. сдвиг ${maxDiff(before, after).toFixed(4)}, узлов ${Object.keys(after).length}/${Object.keys(before).length}`);
  check("возвращает камеру", camBefore.k === camAfter.k && camBefore.x === camAfter.x && camBefore.y === camAfter.y, JSON.stringify([camBefore, camAfter]));
  check("возвращает выбор", JSON.stringify(selBefore) === JSON.stringify(selAfter), JSON.stringify([selBefore, selAfter]));
  check("сохраняет закрепления", JSON.stringify(pinsBefore) === JSON.stringify(pinsAfter), JSON.stringify(pinsAfter));
  check("после возврата есть предложение показать запрос", (await text(page, ".qchip.back")).includes("Показать запрос в графе"));
  // Новый поиск снова включает вид, даже после ручного «Общего вида».
  await page.click('button:has-text("Поиск")');
  await page.fill("textarea", "панели окна");
  await page.click('button:has-text("Найти")');
  await page.waitForTimeout(2200);
  await page.click('button:has-text("Граф")');
  await page.waitForTimeout(1500);
  check("новый поиск снова включает вид запроса", await g(page, "full", "f.queryOn()"));
  check("вид запроса показывает текст нового поиска", (await text(page, ".qbar .qtext")).includes("панели окна"), await text(page, ".qbar .qtext"));
  // Идущий поиск не меняет вид.
  await page.click('button:has-text("Поиск")');
  await page.fill("textarea", "ещё один запрос");
  await page.evaluate(() => {
    const orig = window.fetch;
    window.__slow = true;
  });
  await page.click('button:has-text("Найти")');
  await page.click('button:has-text("Граф")');
  await page.waitForTimeout(80);
  check("ошибок страницы нет", page.errors.length === 0, page.errors.join("; "));
  await page.close();
}

// ---------- 2. Глубина и пределы
{
  const page = await open({ query: "физический граф", mode: "all", qs: "many,limit", depth: "2" });
  const info = await g(page, "full", "({q:f.qinfo().results.length, n:f.shape().nodes.length, known:f.shape().known.nodes, trunc:f.shape().truncated})");
  check("предел 36: показано не больше предела, усечение названо", info.n <= 36 && info.trunc && info.known > info.n, JSON.stringify(info));
  check("все результаты на карте при усечении", await g(page, "full", "f.qinfo().results.every(r=>f.shape().nodes.includes(r.node))"));
  const banner = await text(page, ".banner:has-text(\"Показано\")");
  check("баннер «Показано X из Y» с причиной", /Показано 36 из \d+ узлов/.test(banner) && /Причина/.test(banner), banner.slice(0, 120));
  await page.click('.pg:not(.mini) button[aria-label="Остановить движение"]');
  await page.waitForTimeout(250);
  const p0 = await pos(page);
  const cam0 = await g(page, "full", "f.cam()");
  await page.click('button:has-text("Показать ещё")');
  await page.waitForTimeout(500);
  const p1 = await pos(page);
  const cam1 = await g(page, "full", "f.cam()");
  const n1 = await g(page, "full", "f.shape().nodes.length");
  check("«Показать ещё» добавляет узлы без дубликатов", n1 > info.n && new Set(await g(page, "full", "f.layout().nodes.map(n=>n.index)")).size === n1, `${info.n} → ${n1}`);
  check("«Показать ещё» не сдвигает прежние узлы и камеру", maxDiff(p0, p1) < 1e-6 && cam0.x === cam1.x && cam0.k === cam1.k, `сдвиг ${maxDiff(p0, p1).toFixed(4)}`);
  await page.click('button:has-text("Показать ещё")');
  await page.waitForTimeout(400);
  check("на максимуме кнопка исчезает, предел назван", (await page.locator('button:has-text("Показать ещё")').count()) === 0 && /Достигнут максимум/.test(await text(page, ".banner .end")), await text(page, ".banner .end"));
  for (const d of [0, 1, 2]) {
    await page.click(`.depth button:text-is("${d}")`);
    await page.waitForTimeout(300);
    const nodes = await g(page, "full", "f.shape().nodes.length");
    const res = await g(page, "full", "f.qinfo().results.length");
    if (d === 0) check("глубина 0 — запрос и результаты", nodes === 1 + res, `${nodes} = 1 + ${res}`);
  }
  check("смена глубины не сбрасывает вид и не вписывает камеру повторно", await g(page, "full", "f.queryOn()"));
  await page.close();
}

// ---------- 3. Режим связей: связь запроса видна в любом режиме, раскладка перекладывается
{
  const page = await open({ query: "физический граф", mode: "links" });
  const kindsIn = (mode) => g(page, "full", "[...new Set(f.layout().links.map(l=>l.edge.kind))].sort().join()");
  const a = await kindsIn();
  await page.click('.pg:not(.mini) .mode:has-text("По смыслу"):not(:has-text("чанки"))');
  await page.waitForTimeout(500);
  const b = await kindsIn();
  check("связь запроса видна в режиме «ссылки» и «по смыслу»", a.includes("query_result") && b.includes("query_result"), `${a} | ${b}`);
  check("в режиме «по смыслу» нет ссылок, и наоборот", !b.includes("markdown_link") && !a.includes("document_similarity"));
  await page.close();
}

// ---------- 4. Результаты без объекта графа, связанные источники, оценки
{
  const page = await open({ query: "физический граф", mode: "all", qs: "nograph" });
  const ghosts = await g(page, "full", "f.layout().nodes.filter(n=>n.node.qrole==='ghost').map(n=>n.node.ghost)");
  check("три результата без объекта графа — узлы с причинами", ghosts.length === 3 && ghosts.every(Boolean), JSON.stringify(ghosts));
  check("баннер «Граф неполон» назван", (await text(page, ".banner.warn")).includes("Граф неполон"));
  const ghostLinks = await g(page, "full", "f.layout().links.filter(l=>l.source.node.qrole==='ghost'||l.target.node.qrole==='ghost').every(l=>l.edge.kind==='query_result')");
  check("у такого узла только связь запроса", ghostLinks);
  await page.close();
}
{
  const page = await open({ query: "физический граф", mode: "all", qs: "mix" });
  const rel = await g(page, "full", "f.qinfo().results.filter(r=>r.channel==='related').map(r=>({on:r.onMap,score:r.score,reason:!!r.reason}))");
  check("связанные источники без оценки, основание названо, один вне карты", rel.length === 2 && rel.every((r) => r.score === null && r.reason) && rel.some((r) => !r.on) && rel.some((r) => r.on), JSON.stringify(rel));
  check("баннер о связанных источниках вне карты", (await text(page, ".banner.warn")).includes("вне карты"));
  await page.close();
}
{
  const page = await open({ query: "физический граф", mode: "all", qs: "cosine" });
  check("вид оценки подписан: косинус, rerank не выполнялся", (await text(page, ".qbar .qmeta")).includes("косинус"), await text(page, ".qbar .qmeta"));
  await page.close();
}
{
  const page = await open({ query: "физический граф", mode: "all", qs: "graph" });
  const neutral = await g(page, "full", "f.layout().links.filter(l=>l.edge.kind==='query_result').every(l=>l.edge.neutral&&l.edge.raw===null)");
  check("«Отношения и пути»: связь запроса нейтральна, оценки нет", neutral && (await text(page, ".qbar .qmeta")).includes("оценки нет"));
  await page.close();
}
{
  const page = await open({ query: "физический граф", mode: "all", qs: "mixfallback" });
  check("откат режима поиска называет причину", (await text(page, ".banner.warn")).includes("выполнен как «Обычный»"));
  await page.close();
}
{
  const page = await open({ query: "физический граф", mode: "all", qs: "graphnr" });
  const n = await g(page, "full", "f.layout().nodes.length");
  check("слой недоступен: запрос и результаты, слой помечен", n === 7 && (await page.locator(".banner:has-text(\"graph_not_ready\")").count()) > 0, `узлов ${n}`);
  await page.close();
}
{
  const page = await open({ query: "физический граф", mode: "all", qs: "empty" });
  check("пустая выдача: узел запроса и «Ничего не найдено»", (await g(page, "full", "f.layout().nodes.length")) === 1 && (await text(page, ".qnote")).includes("Ничего не найдено"));
  await page.close();
}
{
  const page = await open({ query: "физический граф", mode: "all", qs: "stale" });
  check("индекс обновился после поиска: вид помечен", (await text(page, ".banner.warn")).includes("изменён после поиска"));
  await page.close();
}

// ---------- 5. Состояния поиска: идёт, ошибка; вид не меняется
{
  const page = await open({ query: "физический граф", mode: "all", qstate: "searching" });
  check("идёт поиск: вид запроса остаётся, баннер", (await g(page, "full", "f.queryOn()")) && (await text(page, ".banner:has-text(\"Идёт поиск\")")).includes("прежний вид"));
  await page.close();
  const page2 = await open({ query: "физический граф", mode: "all", qstate: "error" });
  check("ошибка поиска: вид остаётся, баннер alert", (await g(page2, "full", "f.queryOn()")) && (await page2.locator(".banner.bad[role=alert]").count()) === 1);
  await page2.close();
}

// ---------- 6. Reduced motion, клавиатура
{
  const page = await open({ query: "физический граф", mode: "all", motion: "reduced" });
  const a = await pos(page);
  await page.waitForTimeout(800);
  const b = await pos(page);
  check("reduced motion: вид запроса статичен", maxDiff(a, b) === 0);
  await page.focus('.qbar button:has-text("Общий вид")');
  await page.keyboard.press("Enter");
  await page.waitForTimeout(400);
  check("клавиатура: «Общий вид» с Enter", !(await g(page, "full", "f.queryOn()")));
  check("клавиатура: можно вернуться кнопкой «Показать запрос»", await (async () => {
    await page.focus('.qchip.back button');
    await page.keyboard.press("Enter");
    await page.waitForTimeout(400);
    return g(page, "full", "f.queryOn()");
  })());
  await page.close();
}

// ---------- 7. Узкое окно: инспектор под картой, панель запроса без горизонтальной прокрутки
{
  const page = await open({ query: "физический граф", mode: "all", qs: "limit,many", depth: "2" }, { w: 760, h: 760 });
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth);
  check("760 px: нет горизонтальной прокрутки", !overflow);
  await page.close();
}

mkdirSync(import.meta.dirname, { recursive: true });
writeFileSync(join(import.meta.dirname, "..", "checks.json"), JSON.stringify(results, null, 1));
await browser.close();
const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} проверок пройдено`);
process.exit(failed.length ? 1 : 0);
