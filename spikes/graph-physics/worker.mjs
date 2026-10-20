import { forceSimulation, forceLink, forceManyBody, forceCenter } from 'd3-force';
let state, timer, generation;
self.onmessage = ({ data }) => {
  if (data.command === 'stop') {
    clearTimeout(timer);
    self.postMessage({ stopped: true, generation });
    return;
  }
  clearTimeout(timer);
  generation = data.generation;
  const nodes = structuredClone(data.graph.nodes), edges = structuredClone(data.graph.edges);
  state = forceSimulation(nodes).stop().force('link', forceLink(edges).id(n => n.id).distance(45).strength(e => 0.05 + 0.45 * e.weight)).force('charge', forceManyBody().strength(-30)).force('center', forceCenter());
  let ticks = 0;
  function step() {
    // Bounded batches make stop/generation messages observable between batches.
    state.tick(); ticks++;
    if (ticks % 4 === 0) self.postMessage({ generation, ticks, positions: nodes.map(n => [n.x, n.y]) });
    if (ticks < 120) timer = setTimeout(step, 0);
    else self.postMessage({ generation, complete: true });
  }
  step();
};
