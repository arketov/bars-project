// Проверки стенда 2.3.1 (настоящее окно, Chromium). Запуск: node check.mjs, стенд `npm run graph-actions --prefix ui` на :4184.
import { chromium } from "playwright-core";
import { join } from "node:path";
import { writeFileSync } from "node:fs";

const browser = await chromium.launch({ executablePath: join(process.env.LOCALAPPDATA, "ms-playwright/chromium-1228/chrome-win64/chrome.exe"), args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"] });
const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok: !!ok, detail: String(detail) });
  console.log(`${ok ? "ok  " : "FAIL"} ${name}${detail ? ` — ${detail}` : ""}`);
};
async function open(query = {}, { w = 1440, h = 900, settle = 3800 } = {}) {
  const page = await browser.newPage({ viewport: { width: w, height: h } });
  page.errors = [];
  page.on("pageerror", (e) => page.errors.push(String(e)));
  page.on("console", (m) => m.type() === "error" && page.errors.push(m.text() + " " + (m.location()?.url ?? "")));
  await page.goto(`http://localhost:4184/graph-actions.html?${new URLSearchParams({ theme: "light", ...query })}`);
  await page.waitForTimeout(1200);
  await page.click('button:has-text("Граф")');
  await page.waitForTimeout(settle);
  return page;
}
const g = (page, expr) => page.evaluate(`(()=>{const f=window.__graphActions.full; return ${expr}})()`);
const text = (page, sel) => page.textContent(sel).catch(() => "");
const canvas = ".pg:not(.mini) canvas";
const pos = (layoutExpr) => `Object.fromEntries(${layoutExpr}.nodes.map(n=>[n.index,[Math.round(n.x*100)/100,Math.round(n.y*100)/100]]))`;
const maxDiff = (a, b) => Object.keys(a).reduce((m, k) => (b[k] ? Math.max(m, Math.hypot(a[k][0] - b[k][0], a[k][1] - b[k][1])) : m), 0);

// ---------- 1. Уровни одного снимка
{
  const page = await open({ select: "MAIN_IDEA" });
  check("общий вид: уровень «Документы», 66 узлов", (await g(page, "f.level()")) === "documents" && (await g(page, "f.nodeCount()")) === 66, await g(page, "f.nodeCount()"));
  const sel = await g(page, "f.selected().index");
  await page.click('.seg button:has-text("Смешанный")');
  await page.waitForTimeout(800);
  check("смена уровня сохраняет выбранный документ, если он есть на новом уровне", (await g(page, "f.selected()?.index")) === sel);
  check("смешанный: документы и сущности, полоса «экспериментальный»", (await g(page, "f.nodeCount()")) === 66 + 42 && (await text(page, ".tops")).includes("экспериментальный"));
  await page.click('.seg button:has-text("Чанки")');
  await page.waitForTimeout(800);
  check("уровень «Чанки»: предел 500 и усечение с причиной", (await g(page, "f.nodeCount()")) === 500 && (await text(page, ".tops")).includes("чанков показано 500 из 503"));
  await page.click('.tops .banner button:has-text("Показать ещё")');
  await page.waitForTimeout(800);
  check("«Показать ещё» на уровне чанков добавляет оставшиеся", (await g(page, "f.nodeCount()")) === 503);
  await page.focus(canvas);
  await page.keyboard.press("KeyV");
  await page.waitForTimeout(500);
  check("клавиша V — следующий уровень", (await g(page, "f.level()")) === "entities");
  check("на уровне сущностей вместо режима — фильтр предикатов", (await page.$('button:has-text("Предикаты")')) !== null && (await page.$('.seg[aria-label="Режим связей"]')) === null);
  const before = await g(page, "f.linkCount()");
  await page.click('button:has-text("Предикаты")');
  await page.locator('.pop label.chk', { hasText: "supersedes" }).locator("input").uncheck();
  await page.waitForTimeout(500);
  check("фильтр предиката убирает утверждения и показывает, сколько скрыто", (await g(page, "f.linkCount()")) < before && (await text(page, ".tops")).includes("Фильтры скрывают"), `${before} → ${await g(page, "f.linkCount()")}`);
  check("без ошибок в консоли", !page.errors.length, page.errors.join("; "));
  await page.close();
}

// ---------- 2. Локальный граф и точный возврат
{
  const page = await open({ select: "MAIN_IDEA" });
  await page.click('.pg:not(.mini) button[aria-label="Остановить движение"]');
  await page.waitForTimeout(300);
  const before = await g(page, pos("f.general()"));
  const cam = await g(page, "f.cam()");
  const sel = await g(page, "f.selected().index");
  await page.focus(canvas);
  await page.keyboard.press("KeyL");
  await page.waitForTimeout(1200);
  check("L — локальный граф выбранного, центр отмечен", (await g(page, "f.view()")) === "local" && (await g(page, `f.overlay().rings.get(${sel})`)) === "center");
  const d1 = await g(page, "f.nodeCount()");
  await page.click('.vbar .depth button:has-text("2")');
  await page.waitForTimeout(800);
  check("глубина 2 добавляет соседей соседей", (await g(page, "f.nodeCount()")) > d1, `${d1} → ${await g(page, "f.nodeCount()")}`);
  await page.click('.vbar button:has-text("Выбрать весь локальный граф")');
  check("«Выбрать весь локальный граф» отмечает подграф", (await g(page, "f.subgraph().length")) === (await g(page, "f.nodeCount()")));
  await page.click('.sbar button:has-text("Очистить")');
  await page.click('.vbar button:has-text("Общий вид")');
  await page.waitForTimeout(600);
  const after = await g(page, pos("f.layout()"));
  const camAfter = await g(page, "f.cam()");
  check("«Общий вид» возвращает места узлов", maxDiff(before, after) < 0.01, maxDiff(before, after).toFixed(3));
  const selAfter = await g(page, "f.selected()?.index");
  const camDiff = Math.max(Math.abs(cam.x - camAfter.x), Math.abs(cam.y - camAfter.y), Math.abs(cam.k - camAfter.k) * 1000);
  check("«Общий вид» возвращает камеру и выбор", camDiff < 0.5 && selAfter === sel, JSON.stringify({ cam, camAfter, sel, selAfter }));
  await page.close();
}

// ---------- 3. Путь по карте и смысловой путь
{
  const page = await open({ select: "MAIN_IDEA" });
  await page.focus(canvas);
  await page.keyboard.press("KeyP");
  await page.waitForTimeout(300);
  check("P — выбор второй вершины с подсказкой", (await g(page, "f.view()")) === "pick" && (await text(page, ".vbar")).includes("выберите вторую вершину"));
  await page.keyboard.press("Escape");
  check("Escape отменяет выбор второй вершины", (await g(page, "f.view()")) === null);
  await page.keyboard.press("KeyP");
  await page.fill('input[aria-label="Найти узел графа"]', "ADR-0002");
  await page.press('input[aria-label="Найти узел графа"]', "Enter");
  await page.waitForTimeout(600);
  const tv = await g(page, "({k: f.tv().kind, n: f.tv().alts.length, steps: f.tv().alts.map(a=>a.steps.length)})");
  check("путь по карте: до трёх вариантов, первый — кратчайший", tv.k === "path" && tv.n >= 1 && tv.n <= 3 && tv.steps.every((s, i) => i === 0 || s >= tv.steps[0]), JSON.stringify(tv));
  check("подпись «соседство на карте, а не зависимость»", (await text(page, ".tops")).includes("а не зависимость"));
  check("шаги пути пронумерованы на карте", (await g(page, "f.overlay().stepOf.size")) === (await g(page, "f.tv().alts[0].nodes.length")));
  if (tv.n > 1) {
    await page.focus(canvas);
    await page.keyboard.press("BracketRight");
    check("] — следующий вариант", (await g(page, "f.tv().at")) === 1);
  }
  await page.click('.vbar button:has-text("Смысловой путь")');
  await page.waitForTimeout(300);
  check("смысловой путь пуст с оговоркой «не доказывает»", (await text(page, ".inspector")).includes("не доказывает"));
  await page.click('.inspector button:has-text("Со структурными шагами")');
  await page.waitForTimeout(300);
  check("путь со структурными шагами даёт шаги с основаниями", (await g(page, "f.tv().sem.paths.length")) > 0);
  await page.close();
}

// ---------- 4. Влияние — наложение без перекладки
{
  const page = await open({ select: "MAIN_IDEA" });
  await page.click('.pg:not(.mini) button[aria-label="Остановить движение"]');
  await page.waitForTimeout(300);
  const before = await g(page, pos("f.layout()"));
  await page.focus(canvas);
  await page.keyboard.press("KeyI");
  await page.waitForTimeout(600);
  const after = await g(page, pos("f.layout()"));
  check("I — влияние изменения", (await g(page, "f.view()")) === "impact");
  check("влияние — наложение: узлы не сдвигаются", maxDiff(before, after) < 0.01, maxDiff(before, after).toFixed(3));
  const t = await text(page, ".inspector");
  check("ссылки-соседи отдельно с оговоркой о зависимости", t.includes("Ссылки-соседи") && t.includes("не доказанная зависимость"));
  check("пустой смысловой ответ честно назван", t.includes("Документированных зависимостей не найдено"));
  await page.keyboard.press("Backspace");
  check("Backspace — общий вид", (await g(page, "f.view()")) === null);
  await page.close();
}

// ---------- 5. Подграф как область поиска
{
  const page = await open({ select: "MAIN_IDEA" });
  await page.focus(canvas);
  await page.keyboard.press("Shift+Space");
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("Shift+Space");
  await page.waitForTimeout(200);
  check("Shift+пробел добавляет узлы в подграф", (await g(page, "f.subgraph().length")) === 2);
  check("полоса подграфа с числом документов", (await text(page, ".sbar")).includes("2 документа"));
  await page.click('.sbar button:has-text("Искать в подграфе")');
  await page.waitForTimeout(300);
  const scope = await g(page, "f.scope()");
  check("«Искать в подграфе» заменяет область поиска документами подграфа", scope.length === 2, scope.join(", "));
  await page.click('.sbar button:has-text("Вернуть прежнюю область")');
  const back = (await g(page, "f.scope()")).length;
  check("«Вернуть прежнюю область» восстанавливает область", back === 0);
  await page.click('.pg:not(.mini) button[aria-label="Показать списком"]');
  await page.waitForTimeout(200);
  await page.locator(".list li input[type=checkbox]").nth(5).check();
  check("флажок в списке тоже добавляет в подграф", (await g(page, "f.subgraph().length")) === 3);
  await page.close();
}

// ---------- 6. Меню «Анализ»: контекст, темы, аудит
{
  const page = await open();
  await page.click('button:has-text("Анализ ▾")');
  await page.click('.pop button:has-text("Контекст задачи")');
  await page.fill(".inspector textarea", "Что доступно до одобрения хука Codex?");
  await page.click('.inspector button:has-text("Собрать контекст")');
  await page.waitForTimeout(500);
  check("контекст: наложение со значками ролей", (await g(page, "f.view()")) === "context" && (await g(page, "f.overlay().badges.size")) > 0, await g(page, "f.overlay().badges.size"));
  check("контекст: бюджет и режим подписаны", (await text(page, ".vbar")).includes("оценочный"));
  await page.click('button:has-text("Анализ ▾")');
  await page.click('.pop button:has-text("Обзор тем")');
  await page.waitForTimeout(1200);
  check("обзор тем: свёрнутые группы", (await g(page, "f.nodeCount()")) === 9 && (await g(page, "f.layout().nodes.every(n=>n.node.kind==='topic')")));
  await page.locator(".inspector .conns button").first().click();
  await page.waitForTimeout(800);
  check("раскрытие темы показывает её документы, остальные свёрнуты", (await g(page, "f.nodeCount()")) > 9 && (await g(page, "f.layout().nodes.some(n=>n.node.kind==='topic')")));
  check("рёбра между темами агрегированы с числом", (await g(page, "f.layout().links.some(l=>l.edge.kind==='aggregate' && Object.keys(l.edge.count).length>0)")));
  await page.click('.vbar button:has-text("Общий вид")');
  await page.waitForTimeout(600);
  check("возврат из обзора тем — прежние 66 документов", (await g(page, "f.nodeCount()")) === 66);
  await page.click('button:has-text("Анализ ▾")');
  await page.click('.pop button:has-text("Аудит связей")');
  await page.waitForTimeout(800);
  check("аудит: неразрешённые ссылки призрачными узлами", (await g(page, "f.layout().nodes.filter(n=>n.node.kind==='ghost').length")) > 0);
  check("аудит: изоляты и хабы отмечены кольцами", [...(await g(page, "[...f.overlay().rings.values()]"))].some((r) => r === "hub"));
  await page.click('.vbar button:has-text("Общий вид")');
  await page.waitForTimeout(600);
  check("после аудита призраки убраны", (await g(page, "f.layout().nodes.filter(n=>n.node.kind==='ghost').length")) === 0);
  check("без ошибок в консоли", !page.errors.length, page.errors.join("; "));
  await page.close();
}

// ---------- 7. Фильтры и список
{
  const page = await open({ level: "mixed" });
  await page.click('.pg:not(.mini) button[aria-label^="Фильтры карты"]');
  await page.locator(".filters label.chk", { hasText: "сущности" }).locator("input").uncheck();
  await page.waitForTimeout(500);
  check("фильтр типов узлов скрывает сущности и называет число скрытого", (await g(page, "f.nodeCount()")) === 66 && (await text(page, ".tops")).includes("Фильтры скрывают 42 узлов"));
  check("число активных фильтров на кнопке", (await text(page, ".badged i")) === "1");
  await page.click('.filters button:has-text("Сбросить фильтры")');
  await page.waitForTimeout(400);
  check("сброс фильтров", (await g(page, "f.nodeCount()")) === 108);
  await page.click('.pg:not(.mini) button[aria-label="Показать списком"]');
  check("список: вкладки «Узлы», «Связи», «Вид»", (await page.$$(".tabs button")).length === 3);
  await page.click('.tabs button:has-text("Связи")');
  check("вкладка «Связи» перечисляет связи с видом", (await page.$$(".list li")).length > 0);
  check("вкладка «Вид» недоступна без вида", await page.isDisabled('.tabs button:has-text("Вид")'));
  await page.close();
}

// ---------- 8. Состояния GR-05
const SC = [
  ["empty", (p) => text(p, ".stage").then((t) => t.includes("нет документов"))],
  ["notbuilt", (p) => text(p, ".stage").then((t) => t.includes("не построен"))],
  ["nolinks", (p) => text(p, ".stage").then((t) => t.includes("нет допущенных связей"))],
  ["lagging", async (p) => { await p.click('.seg button:has-text("Сущности")'); return (await text(p, ".tops")).includes("Извлечение отстаёт"); }],
  ["fileerrors", (p) => text(p, ".inspector").then((t) => t.includes("Не обработаны файлы"))],
  ["unresolved", (p) => g(p, "f.layout().nodes.some(n=>n.node.kind==='ghost')")],
  ["ambiguous", async (p) => { await p.click('.seg button:has-text("Сущности")'); await p.waitForTimeout(400); return g(p, "f.layout().nodes.some(n=>n.node.ambiguous)"); }],
  ["conflict", async (p) => { await p.click('.seg button:has-text("Сущности")'); await p.waitForTimeout(400); return g(p, "[...f.overlay().edgeMeta.values()].some(m=>m.conflict)"); }],
  ["conditions", async (p) => { await p.click('.seg button:has-text("Сущности")'); await p.waitForTimeout(400); return g(p, "[...f.overlay().edgeMeta.values()].some(m=>m.cond)"); }],
  ["limit", async (p) => { await g(p, "f.select(f.find('MAIN_IDEA'))"); await p.focus(canvas); await p.keyboard.press("KeyL"); await p.waitForTimeout(600); return (await text(p, ".tops")).includes("Достигнут предел обхода"); }],
  ["stale", (p) => text(p, ".tops").then((t) => t.includes("изменены после снимка"))],
  ["deleted", async (p) => { await g(p, "f.select(f.find('Уведомления и колокольчик'))"); await p.waitForTimeout(200); return p.isDisabled('.inspector button:has-text("Открыть")'); }],
  ["norender", (p) => text(p, ".stage").then((t) => t.includes("Canvas недоступен"))],
];
for (const [scenario, probe] of SC) {
  const page = await open({ scenario });
  check(`состояние ${scenario}`, await probe(page), page.errors.join("; "));
  await page.close();
}

// ---------- 9. Reduced motion, узкое окно, тёмная тема
{
  const page = await open({ motion: "reduced", select: "MAIN_IDEA" });
  check("reduced motion: движения нет, пауза недоступна", (await text(page, ".pg:not(.mini) .status")).includes("reduced motion") && (await page.isDisabled('.pg:not(.mini) button[aria-label="Остановить движение"]')));
  await page.focus(canvas);
  await page.keyboard.press("KeyL");
  await page.waitForTimeout(400);
  check("reduced motion: локальный граф открывается без анимации", (await g(page, "f.view()")) === "local" && !(await g(page, "f.layout().active")));
  await page.close();
}
{
  const page = await open({ select: "MAIN_IDEA", view: "path", to: "ADR-0002" }, { w: 760, h: 900, settle: 4500 });
  const box = await page.evaluate(() => { const i = document.querySelector(".inspector").getBoundingClientRect(); const s = document.querySelector(".pg:not(.mini) .stage").getBoundingClientRect(); return { below: i.top >= s.bottom - 1, overflow: document.documentElement.scrollWidth > innerWidth }; });
  check("узкое окно: инспектор под картой, без горизонтальной прокрутки", box.below && !box.overflow, JSON.stringify(box));
  await page.close();
}

const failed = results.filter((r) => !r.ok).length;
console.log(`\n${results.length - failed} из ${results.length}`);
writeFileSync(join(import.meta.dirname, "..", "checks.json"), JSON.stringify({ date: "2026-10-22", passed: results.length - failed, total: results.length, results }, null, 2));
await browser.close();
process.exit(failed ? 1 : 0);
