// Замер раскладок d3-force на снимке bars-project: насколько связанные узлы держатся цепочками, а одиночки — не слипаются.
// Запуск из корня репозитория: node docs/chats/2026-10-21-graph-design/tools/physics-bench.mjs
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
const require = createRequire(new URL("../../../../ui/package.json", import.meta.url));
const { forceSimulation, forceLink, forceManyBody, forceCollide, forceX, forceY } = require("d3-force");
const fx = JSON.parse(readFileSync(new URL("../../../../ui/src/spikes/graph-design/fixture.json", import.meta.url), "utf8"));

const springOf = (s) => 0.05 + 0.45 * s;
function edgesFor(mode, smin) {
  const out = [];
  if (mode !== "meaning") for (const [a, b] of fx.links) out.push({ a, b, s: 1 });
  if (mode !== "links") for (const [a, b, w] of fx.dsim) if (w >= smin) out.push({ a, b, s: w });
  return out;
}
function coefficients(edges) {
  const buckets = new Map();
  for (const e of edges) {
    const key = e.a < e.b ? `${e.a}:${e.b}` : `${e.b}:${e.a}`;
    buckets.set(key, [...(buckets.get(key) ?? []), e]);
  }
  for (const list of buckets.values()) {
    const q = list.map((e) => springOf(e.s));
    const total = q.reduce((x, y) => x + y, 0);
    const max = Math.max(...q);
    list.forEach((e, i) => (e.k = (max * q[i]) / total));
  }
}

function run(mode, cfg) {
  const edges = edgesFor(mode, cfg.smin ?? 0.82);
  coefficients(edges);
  const n = fx.docs.length;
  const nodes = Array.from({ length: n }, (_, i) => ({ id: i, degree: 0 }));
  for (const e of edges) {
    nodes[e.a].degree++;
    nodes[e.b].degree++;
  }
  const links = edges.map((e) => ({ source: nodes[e.a], target: nodes[e.b], k: e.k, s: e.s }));
  let seed = 7;
  const rnd = () => ((seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0) / 4294967296);
  const strength = (l) => {
    const deg = Math.max(1, Math.min(l.source.degree, l.target.degree));
    if (cfg.strength === "d3") return (cfg.linkForce ?? 1) / deg;
    if (cfg.strength === "raw") return Math.min(1, (cfg.linkForce ?? 1) * l.k);
    return Math.min(1, ((cfg.linkForce ?? 1) * l.k) / deg);
  };
  const sim = forceSimulation(nodes)
    .randomSource(rnd)
    .alphaDecay(cfg.alphaDecay ?? 0.0228)
    .velocityDecay(cfg.velocityDecay ?? 0.4)
    .force("link", forceLink(links).strength(strength).distance(cfg.distance ?? 60))
    .force("charge", forceManyBody().strength((d) => (d.degree === 0 ? (cfg.isoCharge ?? cfg.charge ?? -90) : (cfg.charge ?? -90))).distanceMax(cfg.distanceMax ?? 420))
    .force("collide", forceCollide(7.5))
    .force("x", forceX(0).strength(cfg.gravity ?? 0.015))
    .force("y", forceY(0).strength(cfg.gravity ?? 0.015));
  if (cfg.far) sim.force("far", forceManyBody().strength(cfg.far).distanceMax(5000));
  sim.stop();
  const ticks = cfg.ticks ?? 700;
  sim.alpha(1);
  for (let i = 0; i < ticks; i++) sim.tick();
  // метрики
  const nn = (i, pool) => Math.min(...pool.filter((j) => j !== i).map((j) => Math.hypot(nodes[i].x - nodes[j].x, nodes[i].y - nodes[j].y)));
  const connected = nodes.filter((d) => d.degree > 0).map((d) => d.id);
  const isolated = nodes.filter((d) => d.degree === 0).map((d) => d.id);
  const lens = links.map((l) => Math.hypot(l.source.x - l.target.x, l.source.y - l.target.y)).sort((a, b) => a - b);
  const med = (xs) => (xs.length ? xs.slice().sort((a, b) => a - b)[Math.floor(xs.length / 2)] : 0);
  const L = cfg.distance ?? 60;
  const r = Math.max(...nodes.map((d) => Math.hypot(d.x, d.y)));
  const nearLinked = lens.filter((x) => x < 1.6 * L).length / (lens.length || 1);
  return {
    edges: edges.length,
    isolated: isolated.length,
    medLinkLen: Math.round(med(lens)),
    shortShare: Math.round(nearLinked * 100) / 100,
    medNNconnected: Math.round(med(connected.map((i) => nn(i, connected)))),
    medNNisolated: isolated.length > 1 ? Math.round(med(isolated.map((i) => nn(i, isolated)))) : 0,
    radius: Math.round(r),
    coreR90: Math.round(connected.map((i) => Math.hypot(nodes[i].x, nodes[i].y)).sort((a, b) => a - b)[Math.floor(connected.length * 0.9)] ?? 0),
    isoMedR: Math.round(med(isolated.map((i) => Math.hypot(nodes[i].x, nodes[i].y)))),
    alphaAfter: Math.round(sim.alpha() * 1000) / 1000,
  };
}

const configs = JSON.parse(process.argv[2] ?? "[]");
for (const [name, mode, cfg] of configs) console.log(name.padEnd(34), mode.padEnd(6), JSON.stringify(run(mode, cfg)));
