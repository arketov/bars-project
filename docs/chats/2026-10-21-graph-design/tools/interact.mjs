import { chromium } from "playwright-core";
import { join } from "node:path";
const browser = await chromium.launch({ executablePath: join(process.env.LOCALAPPDATA, "ms-playwright/chromium-1228/chrome-win64/chrome.exe"), args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"] });
const results = [];
const check = (name, ok, detail = "") => results.push({ name, ok, detail });
async function open(query = {}, w = 1440, h = 900) {
  const page = await browser.newPage({ viewport: { width: w, height: h } });
  page.errors = [];
  page.on("pageerror", (e) => page.errors.push(String(e)));
  await page.goto(`http://localhost:4181/graph-design.html?${new URLSearchParams({ theme: "light", ...query })}`);
  await page.waitForTimeout(1500);
  await page.click('button:has-text("Граф")');
  await page.waitForTimeout(2500);
  return page;
}
const full = (page, expr) => page.evaluate(`(()=>{const f=window.__graphDesign.full; return ${expr}})()`);
const mini = (page, expr) => page.evaluate(`(()=>{const f=window.__graphDesign.mini; return ${expr}})()`);
const snapshot = (page) => full(page, "f.layout().nodes.map(n=>[n.index,Math.round(n.x*100)/100,Math.round(n.y*100)/100])");
const diff = (a, b) => a.reduce((m, [, x, y], i) => Math.max(m, Math.hypot(x - b[i][1], y - b[i][2])), 0);
const kindsOf = (page, which) => page.evaluate((w) => [...new Set(window.__graphDesign[w].layout().links.map((l) => l.edge.kind))].sort().join(), which);
// Прогнать физику до покоя; вернуть долю связей короче 1,6 длины и медиану длины связи.
const settle = (page, which = "full") =>
  page.evaluate((w) => {
    const l = window.__graphDesign[w].layout();
    l.sim.alpha(0.9);
    for (let i = 0; i < 1500; i++) l.sim.tick();
    const len = l.forces.length;
    const lens = l.links.filter((k) => k.edge.kind !== "query_result").map((k) => Math.hypot(k.source.x - k.target.x, k.source.y - k.target.y)).sort((a, b) => a - b);
    return { share: lens.length ? lens.filter((x) => x < 1.6 * len).length / lens.length : 1, med: lens[Math.floor(lens.length / 2)] ?? 0 };
  }, which);

// ---------- общий вид, клавиатура, остановка, камера, список
let page = await open({ select: "MAIN_IDEA" });
await page.focus(".pg:not(.mini) canvas");
const title = await page.textContent("aside.inspector h3");
check("инспектор показывает выбранный узел", !!title && title.length > 3, title);
await page.keyboard.press("Enter");
await page.waitForTimeout(800);
const status = await page.textContent(".pg:not(.mini) .stage .status");
check("Enter раскрывает чанки выбранного документа", /чанков \d+ из/.test(status), status);
await page.keyboard.press("ArrowRight");
const sel1 = await full(page, "f.selected()");
check("стрелка выбирает соседний узел", sel1?.type === "node", JSON.stringify(sel1));
await page.keyboard.press("Space");
await page.waitForTimeout(150);
check("пробел закрепляет выбранный узел", (await full(page, "f.layout().nodes.filter((n) => n.pinned).length")) === 1);
await page.click('.pg:not(.mini) button[aria-label="Остановить движение"]');
const a = await snapshot(page);
await page.waitForTimeout(1200);
const b = await snapshot(page);
check("после остановки узлы не двигаются", diff(a, b) === 0, `макс. сдвиг ${diff(a, b)}`);
check("статус «Остановлено»", /Остановлено/.test(await page.textContent(".pg:not(.mini) .stage .status")));
await page.focus(".pg:not(.mini) canvas");
await page.keyboard.press("Escape");
await page.keyboard.press("ArrowLeft");
await page.keyboard.press("Enter");
await page.waitForTimeout(600);
const c = await snapshot(page);
const byIndex = new Map(c.map(([i, x, y]) => [i, [x, y]]));
const moved = a.filter(([i, x, y]) => byIndex.has(i) && Math.hypot(byIndex.get(i)[0] - x, byIndex.get(i)[1] - y) > 0).length;
check("раскрытие при остановке не сдвигает прежние узлы", moved === 0, `сдвинуто ${moved} из ${a.length}`);
await page.click('.pg:not(.mini) button[aria-label="Вписать"]');
const cam0 = await full(page, "f.cam()");
await page.click('button[aria-label="Увеличить масштаб"]');
const cam1 = await full(page, "f.cam()");
check("масштаб меняет камеру", cam1.k > cam0.k, `${cam0.k} -> ${cam1.k}`);
const stageBox = () => page.evaluate(() => { const r = document.querySelector(".pg:not(.mini) .stage").getBoundingClientRect(); return [r.width, r.height]; });
const center = (cam, box) => [(box[0] / 2 - cam.x) / cam.k, (box[1] / 2 - cam.y) / cam.k];
const before = center(cam1, await stageBox());
await page.setViewportSize({ width: 1100, height: 760 });
await page.waitForTimeout(800);
const cam2 = await full(page, "f.cam()");
const after = center(cam2, await stageBox());
check("resize сохраняет масштаб и мировой центр", cam2.k === cam1.k && Math.hypot(before[0] - after[0], before[1] - after[1]) < 2, `k ${cam1.k}/${cam2.k}`);
await page.click('button[aria-label="Показать списком"]');
const rows = await page.locator(".list li").count();
check("список показывает узлы", rows > 100, `${rows}`);
await page.fill('input[aria-label="Фильтр списка узлов"]', "MAIN_IDEA");
await page.waitForTimeout(200);
check("фильтр списка", (await page.locator(".list li").count()) < rows);
await page.click('button[aria-label="Показать карту"]');
await page.click('button[aria-label="Легенда и шкала силы"]');
check("легенда открывается", await page.locator("section.legend").isVisible());
await page.click('section.legend button:has-text("Проект 2.1.3")');
check("переключение шкалы", (await full(page, "f.scale()")) === "project");
await page.click(".pg:not(.mini) .mode >> nth=1");
check("полный граф: режим «По смыслу» — только близость документов", (await kindsOf(page, "full")) === "document_similarity", await kindsOf(page, "full"));
await page.click(".pg:not(.mini) .mode >> nth=3");
const allKinds = await kindsOf(page, "full");
check("полный граф: «Все связи» — ссылки и близость вместе", allKinds.includes("markdown_link") && allKinds.includes("document_similarity"), allKinds);
await page.evaluate(() => { window.__ref = window.__graphDesign.full.layout(); });
await page.click('.dv-tab:has-text("Граф") .dv-default-tab-action');
await page.waitForTimeout(600);
check("закрытие панели уничтожает layout", await page.evaluate(() => window.__ref.nodes.length === 0));
check("без ошибок страницы", page.errors.length === 0, page.errors.join(" | "));
await page.close();

// ---------- reduced motion и скрытое окно
page = await open({ motion: "reduced" });
const r1 = await snapshot(page);
await page.waitForTimeout(1500);
const r2 = await snapshot(page);
check("reduced motion: позиции статичны", diff(r1, r2) === 0 && (await page.textContent(".pg:not(.mini) .stage .status")).includes("reduced"));
check("reduced motion: кнопка остановки недоступна", await page.locator('.pg:not(.mini) button[aria-label="Остановить движение"]').isDisabled());
await page.close();
page = await open();
await page.evaluate(() => { Object.defineProperty(document, "hidden", { value: true, configurable: true }); document.dispatchEvent(new Event("visibilitychange")); });
const h1 = await snapshot(page);
await page.waitForTimeout(800);
check("скрытое окно: движение не идёт", diff(h1, await snapshot(page)) === 0);
await page.close();

// ---------- физика: нет «молока», нет раннего замораживания, цепочки образуются
page = await open();
const alphaAt4s = await full(page, "f.layout().sim.alpha()");
check("через 4 с раскладка ещё движется (не замёрзла рано)", alphaAt4s > 0.005, `alpha ${alphaAt4s.toFixed(4)}`);
const s1 = await snapshot(page);
await page.waitForTimeout(400);
const moving = diff(s1, await snapshot(page));
check("узлы заметно скользят, а не вязнут", moving > 0.5, `макс. сдвиг за 0,4 с ${moving.toFixed(1)}`);
const linksChain = await settle(page);
check("режим ссылок: связи собирают цепочки", linksChain.share >= 0.7, linksChain.share.toFixed(2));
await page.close();
page = await open({ mode: "meaning" });
const meaningChain = await settle(page);
check("режим «по смыслу»: цепочки образуются", meaningChain.share >= 0.85, meaningChain.share.toFixed(2));
await page.evaluate(() => window.__graphDesign.full.layout().setForces({ link: 4, repel: 4, gravity: 1, length: 60 }));
const maxChain = await settle(page);
check("максимум притяжения и отталкивания одновременно: цепочки не рвутся", maxChain.share >= 0.5, maxChain.share.toFixed(2));
const radius = (g) => page.evaluate((grav) => { const l = window.__graphDesign.full.layout(); l.setForces({ link: 1, repel: 1, gravity: grav, length: 60 }); l.sim.alpha(0.9); for (let i = 0; i < 1500; i++) l.sim.tick(); const iso = l.nodes.filter((n) => n.degree === 0); return iso.reduce((s, n) => s + Math.hypot(n.x, n.y), 0) / iso.length; }, g);
const gr4 = await radius(4);
const gr0 = await radius(0);
check("«общее притяжение» стягивает одиночки к центру", gr4 < gr0, `радиус одиночек ×4: ${Math.round(gr4)} против ×0: ${Math.round(gr0)}`);
await page.close();

// ---------- режимы не влияют друг на друга: после смены режима раскладка такая же, как при прямом открытии
page = await open({ mode: "meaning" });
const direct = await settle(page);
await page.close();
page = await open();
await page.click(".pg:not(.mini) .mode >> nth=1");
await page.waitForTimeout(500);
const switched = await settle(page);
const rel = Math.abs(switched.med - direct.med) / direct.med;
check("после смены режима раскладка не хранит следов прошлого режима", rel < 0.2 && switched.share >= 0.85, `медиана связи ${Math.round(switched.med)} против ${Math.round(direct.med)}, цепочки ${switched.share.toFixed(2)}`);
const physKinds = await full(page, "[...new Set(f.layout().links.map((l) => l.edge.kind))].join()");
check("в физике режима только его связи", physKinds === "document_similarity", physKinds);
await page.close();

// ---------- настоящие ссылки
page = await open();
const linkInfo = await full(page, "(() => { const e = f.slice().edges.filter((x) => x.kind === 'markdown_link'); return { n: e.length, withLines: e.filter((x) => x.lines.length > 0).length, mutual: e.filter((x) => e.some((y) => y.a === x.b && y.b === x.a)).length }; })()");
check("все ссылки графа имеют строки в тексте (нет придуманных)", linkInfo.n > 500 && linkInfo.withLines === linkInfo.n, JSON.stringify(linkInfo));
await page.close();

// ---------- меню мини-графа, режимы, ползунки
page = await open();
await page.hover(".mini .pg");
check("мини-граф стартует в режиме ссылок", (await kindsOf(page, "mini")) === "markdown_link");
await page.click(".mini button.mode-btn");
check("кнопка режима: ссылки → по смыслу", (await kindsOf(page, "mini")) === "document_similarity", await kindsOf(page, "mini"));
const toastText = await page.textContent(".mini .toast");
check("после переключения всплывает режим", /по смыслу/i.test(toastText), toastText);
await page.click(".mini button.mode-btn");
check("третий режим — все связи вместе", (await kindsOf(page, "mini")) === "document_similarity,markdown_link", await kindsOf(page, "mini"));
await page.click(".mini button.mode-btn");
check("цикл режимов замыкается на ссылках", (await kindsOf(page, "mini")) === "markdown_link");
await page.waitForTimeout(2800);
check("подсказка режима исчезает сама", (await page.locator(".mini .toast").count()) === 0);
const levelLabel = await page.textContent(".mini .status");
check("мини-граф подписан: «Документы»", /Документы 301/.test(levelLabel), levelLabel);
await page.click('.mini button[aria-label="Настройки раскладки"]');
check("меню мини-графа: настройки открываются", await page.locator('.mini section.pop[aria-label="Настройки раскладки"]').isVisible());
await page.locator('.mini input[aria-label="Притяжение по связям"]').fill("2");
await page.locator('.mini input[aria-label="Отталкивание узлов"]').fill("1.5");
await page.locator('.mini input[aria-label="Общее притяжение узлов"]').fill("2");
await page.locator('.mini input[aria-label="Длина связи"]').fill("90");
const forces = await mini(page, "f.forces()");
check("четыре ползунка меняют раскладку", forces.link === 2 && forces.repel === 1.5 && forces.gravity === 2 && forces.length === 90, JSON.stringify(forces));
const applied = await mini(page, "(() => { const l = f.layout(); return [l.forces.link, l.forces.length, l.nodes[0].charge]; })()");
check("настройки применены к физике", applied[0] === 2 && applied[1] === 90 && Math.abs(applied[2]) > 80, JSON.stringify(applied));
await page.click('.mini section.pop button:has-text("По умолчанию")');
const reset = await mini(page, "f.forces()");
check("сброс настроек раскладки", reset.link === 1 && reset.repel === 1 && reset.gravity === 1 && reset.length === 60);
await page.screenshot({ path: join(import.meta.dirname, "shots", "mini-menu.png"), clip: { x: 0, y: 520, width: 260, height: 380 } });
await page.close();

// ---------- граф от запроса
page = await open({ query: "граф связи документов окно" });
const qinfo = await full(page, "(() => { const s = f.slice(); return { q: !!s.queryInfo, results: s.queryInfo?.results.length, nodes: f.layout().nodes.filter((n) => n.node.kind === 'query').length, qlinks: f.layout().links.filter((l) => l.edge.kind === 'query_result').length, docs: f.layout().nodes.length }; })()");
check("запрос показан точкой с рёбрами к результатам", qinfo.q && qinfo.nodes === 1 && qinfo.qlinks === qinfo.results && qinfo.results >= 1, JSON.stringify(qinfo));
check("к результатам добавлены связанные документы", qinfo.docs > qinfo.results + 1, JSON.stringify(qinfo));
const chip = await page.textContent(".pg:not(.mini) .qchip");
check("плашка запроса видна в полном графе", /Запрос/.test(chip) && /граф связи/.test(chip), chip);
check("мини-граф тоже показывает запрос", (await mini(page, "f.layout().nodes.some((n) => n.node.kind === 'query')")) === true);
await page.click(".pg:not(.mini) .mode >> nth=1");
await page.waitForTimeout(300);
const meaningKinds = await kindsOf(page, "full");
check("в виде запроса режим связей переключает связи результатов", meaningKinds === "document_similarity,query_result", meaningKinds);
await page.click(".pg:not(.mini) .qchip button");
await page.waitForTimeout(500);
check("«Общий вид» возвращает весь проект", (await full(page, "f.layout().nodes.length")) === 301);
await page.close();

await browser.close();
console.log(JSON.stringify(results, null, 1));
console.log(results.every((r) => r.ok) ? "ALL OK" : "FAILED: " + results.filter((r) => !r.ok).map((r) => r.name).join("; "));
