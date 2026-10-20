import cytoscape from 'cytoscape';
import { forceSimulation, forceLink, forceManyBody, forceCenter, forceX, forceY, forceCollide } from 'd3-force';
import Graph from 'graphology';
import fa2 from 'graphology-layout-forceatlas2';
import Sigma from 'sigma';
import ForceWorker from './worker.mjs?worker';
const clone = structuredClone;
const frame = () => new Promise(requestAnimationFrame);
const quantile = (xs, p) => [...xs].sort((a, b) => a - b)[Math.floor((xs.length - 1) * p)];
function graphology(g) {
  const graph = new Graph({ multi: true, type: 'undirected' });
  for (const [i, n] of g.nodes.entries()) graph.addNode(n.id, { ...n, size: 3, color: n.kind === 'document' ? '#e14b89' : '#828a98', label: i < 30 ? n.label : '' });
  for (const [i, e] of g.edges.entries()) graph.addEdgeWithKey(String(i), e.source, e.target, { weight: e.weight, size: 0.5 + e.weight, color: '#8b899366' });
  return graph;
}
function cyElements(g) {
  return [...g.nodes.map((n, i) => ({ data: { id: n.id, label: i < 30 ? n.label : '' }, position: { x: n.x, y: n.y } })), ...g.edges.map((e, i) => ({ data: { id: 'e' + i, ...e } }))];
}
function d3(g) {
  const nodes = clone(g.nodes), edges = clone(g.edges);
  const sim = forceSimulation(nodes).stop().force('link', forceLink(edges).id(n => n.id).distance(45).strength(e => 0.05 + 0.45 * e.weight)).force('charge', forceManyBody().strength(-30)).force('center', forceCenter());
  return { nodes, sim };
}
window.physics = (g, method) => {
  const start = performance.now();
  let positions, destroy;
  if (method === 'cose') {
    const cy = cytoscape({ headless: true, styleEnabled: false, elements: cyElements(g) });
    // CoSE divides spring force by elasticity: smaller elasticity means stronger pull.
    cy.layout({ name: 'cose', animate: false, randomize: false, fit: false, numIter: 120, refresh: 0, initialTemp: 100, minTemp: 0, coolingFactor: 0.99, edgeElasticity: e => 200 - 180 * e.data('weight') }).run();
    positions = cy.nodes().map(n => [n.position().x, n.position().y]);
    destroy = () => cy.destroy();
  } else if (method === 'd3') {
    const { nodes, sim } = d3(g); sim.tick(120);
    positions = nodes.map(n => [n.x, n.y]); destroy = () => sim.stop();
  } else {
    const graph = graphology(g);
    fa2.assign(graph, { iterations: 120, getEdgeWeight: 'weight', settings: { barnesHutOptimize: true, gravity: 1, scalingRatio: 10, edgeWeightInfluence: 1, slowDown: 2 } });
    positions = graph.nodes().map(id => [graph.getNodeAttribute(id, 'x'), graph.getNodeAttribute(id, 'y')]); destroy = () => graph.clear();
  }
  const elapsedMs = performance.now() - start;
  const finite = positions.every(p => p.every(Number.isFinite)); destroy();
  return { elapsedMs, finite, positions };
};
window.render = async (g, method, width, height) => {
  const container = document.querySelector('#graph');
  container.style.cssText = `width:${width}px;height:${height}px;position:relative;background:#202127`;
  const start = performance.now(); let paint, destroy, renderer;
  if (method === 'cytoscape') {
    renderer = cytoscape({ container, elements: cyElements(g), layout: { name: 'preset', fit: true, padding: 20 }, style: [{ selector: 'node', style: { width: 6, height: 6, 'background-color': '#e14b89', color: '#ddd', label: 'data(label)', 'font-size': 10 } }, { selector: 'edge', style: { width: 'mapData(weight,0,1,0.5,1.5)', opacity: 0.4, 'line-color': '#8b8993', 'curve-style': 'straight' } }] });
    paint = i => renderer.pan({ x: renderer.pan().x + Math.sin(i / 8), y: renderer.pan().y });
    destroy = () => renderer.destroy();
  } else if (method === 'sigma') {
    const graph = graphology(g);
    renderer = new Sigma(graph, container, { renderLabels: true, labelDensity: 1, labelColor: { color: '#ddd' }, allowInvalidContainer: false });
    paint = i => { const camera = renderer.getCamera(); camera.setState({ x: camera.x + Math.sin(i / 8) / width }); };
    destroy = () => renderer.kill();
  } else {
    const canvas = document.createElement('canvas'); canvas.width = width * devicePixelRatio; canvas.height = height * devicePixelRatio; canvas.style.cssText = `width:${width}px;height:${height}px`; container.append(canvas);
    const ctx = canvas.getContext('2d'), lookup = new Map(g.nodes.map(n => [n.id, n]));
    const span = Math.max(...g.nodes.map(n => Math.max(Math.abs(n.x), Math.abs(n.y))), 1);
    const scale = Math.min(width, height) / (span * 2.2);
    paint = i => {
      ctx.setTransform(devicePixelRatio, 0, 0, devicePixelRatio, 0, 0); ctx.clearRect(0, 0, width, height); ctx.translate(width / 2 + Math.sin(i / 8) * 8, height / 2); ctx.scale(scale, scale);
      for (const e of g.edges) { const a = lookup.get(e.source), b = lookup.get(e.target); ctx.strokeStyle = `rgba(139,137,147,${0.15 + 0.35 * e.weight})`; ctx.lineWidth = (0.5 + e.weight) / scale; ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke(); }
      for (const [i, n] of g.nodes.entries()) { ctx.fillStyle = '#e14b89'; ctx.beginPath(); ctx.arc(n.x, n.y, 3 / scale, 0, 2 * Math.PI); ctx.fill(); if (i < 30) { ctx.fillStyle = '#ddd'; ctx.font = `${10 / scale}px sans-serif`; ctx.fillText(n.label, n.x + 5 / scale, n.y); } }
    };
    destroy = () => canvas.remove();
  }
  paint(0); await frame(); await frame();
  const firstFrameMs = performance.now() - start, intervals = [], work = []; let last = performance.now();
  for (let i = 0; i < 90; i++) { const t = performance.now(); paint(i); work.push(performance.now() - t); await frame(); const now = performance.now(); intervals.push(now - last); last = now; }
  const contexts = [...container.querySelectorAll('canvas')].length;
  destroy(); await frame();
  return { firstFrameMs, frameP50Ms: quantile(intervals, 0.5), frameP95Ms: quantile(intervals, 0.95), submitP95Ms: quantile(work, 0.95), contexts, remainingCanvas: container.querySelectorAll('canvas').length };
};
window.weightCheck = () => {
  const result = {};
  for (const method of ['d3', 'fa2', 'cose']) {
    result[method] = [0.1, 0.9].map(weight => {
      const g = { nodes: [{ id: 'a', x: -100, y: 0 }, { id: 'b', x: 100, y: 0 }], edges: [{ source: 'a', target: 'b', weight }] };
      const p = window.physics(g, method).positions;
      return Math.hypot(p[0][0] - p[1][0], p[0][1] - p[1][1]);
    });
  }
  // One tick isolates spring response from the rest of the layout.
  result.d3OneTick = [0.1, 0.9].map(weight => { const { nodes, sim } = d3({ nodes: [{ id: 'a', x: -100, y: 0 }, { id: 'b', x: 100, y: 0 }], edges: [{ source: 'a', target: 'b', weight }] }); sim.tick(); return Math.hypot(nodes[0].x - nodes[1].x, nodes[0].y - nodes[1].y); });
  return result;
};
window.workerCheck = async g => {
  const worker = new ForceWorker(); let updates = 0, stopped = false, late = 0, ticksAtStop;
  const start = performance.now(); let heartbeat = 0;
  const timer = setInterval(() => heartbeat++, 10);
  const result = await new Promise((resolve, reject) => {
    const deadline = setTimeout(() => reject(Error('worker deadline')), 15000);
    worker.onerror = reject;
    worker.onmessage = ({ data }) => {
      if (data.positions) { if (stopped) late++; else updates++; if (updates === 3) { ticksAtStop = data.ticks; worker.postMessage({ command: 'stop' }); } }
      if (data.stopped) { stopped = true; setTimeout(() => { clearTimeout(deadline); resolve({ updates, ticksAtStop, late, heartbeat, elapsedMs: performance.now() - start }); }, 100); }
    };
    worker.postMessage({ command: 'start', graph: g, generation: 1 });
  }).finally(() => { clearInterval(timer); worker.terminate(); });
  return result;
};
window.capture = (g, title) => {
  document.querySelector('h1').textContent = title + ' — исследование, не макет продукта';
  const container = document.querySelector('#graph');
  container.style.cssText = 'width:1000px;height:650px;position:relative;background:#202127';
  const renderer = new Sigma(graphology(g), container, { renderLabels: true, labelColor: { color: '#ddd' } });
  renderer.refresh();
  return { canvases: container.querySelectorAll('canvas').length };
};
window.pinStopCheck = async g => {
  const { nodes, sim } = d3(g);
  nodes[0].fx = 23; nodes[0].fy = -17; sim.tick(20);
  const pinned = nodes[0].x === 23 && nodes[0].y === -17;
  sim.stop(); const before = nodes.map(n => [n.x,n.y]);
  await new Promise(r => setTimeout(r, 100));
  const stopped = nodes.every((n,i) => n.x === before[i][0] && n.y === before[i][1]);
  nodes[0].fx = null; nodes[0].fy = null; sim.alpha(0.5).tick(20);
  const released = nodes[0].x !== 23 || nodes[0].y !== -17;
  sim.stop();
  return { graph: g.name, pinned, stopped, released };
};
window.tunedD3 = g => {
  const nodes = clone(g.nodes), edges = clone(g.edges), degree = new Map(nodes.map(n => [n.id,0]));
  for (const e of edges) { degree.set(e.source,degree.get(e.source)+1); degree.set(e.target,degree.get(e.target)+1); }
  const start = performance.now();
  // Explicit per-edge strength otherwise discards D3's degree stabilisation at hubs.
  const sim = forceSimulation(nodes).stop()
    .force('link',forceLink(edges).id(n=>n.id).distance(45).strength(e=>(0.05+0.45*e.weight)/Math.max(1,Math.min(degree.get(e.source.id),degree.get(e.target.id)))))
    .force('charge',forceManyBody().strength(-90))
    .force('collision',forceCollide(6))
    .force('x',forceX(0).strength(0.015)).force('y',forceY(0).strength(0.015));
  sim.tick(120); sim.stop();
  return { elapsedMs: performance.now()-start, finite: nodes.every(n=>Number.isFinite(n.x)&&Number.isFinite(n.y)), positions: nodes.map(n=>[n.x,n.y]) };
};
