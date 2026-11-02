(() => {
  const stats = { timers: [], frames: 0, rpc: {}, workers: { sent: {}, received: {} } };
  const count = (map, key) => { map[key] = (map[key] ?? 0) + 1; };
  for (const name of ['setInterval', 'setTimeout']) {
    const original = window[name].bind(window);
    window[name] = (callback, delay, ...args) => {
      const row = { kind: name, delay: Number(delay) || 0, fires: 0, stack: new Error().stack.split('\n').slice(2, 7).join('\n') };
      stats.timers.push(row);
      return original(typeof callback === 'function' ? function (...values) { row.fires++; return Reflect.apply(callback, this, values); } : callback, delay, ...args);
    };
  }
  const raf = window.requestAnimationFrame.bind(window);
  window.requestAnimationFrame = callback => raf(function (time) { stats.frames++; return Reflect.apply(callback, this, [time]); });
  const NativeWorker = window.Worker;
  window.Worker = class extends NativeWorker {
    constructor(...args) {
      super(...args);
      this.addEventListener('message', event => count(stats.workers.received, event.data?.t ?? 'unknown'));
    }
    postMessage(message, ...args) { count(stats.workers.sent, message?.t ?? 'unknown'); return super.postMessage(message, ...args); }
  };
  const nativeFetch = window.fetch.bind(window);
  window.fetch = (url, options) => {
    const uri = new URL(typeof url === 'string' ? url : url.url ?? String(url), location.href);
    if (uri.hostname === 'ipc.localhost') {
      let command = decodeURIComponent(uri.pathname.slice(1));
      if (command === 'bars_call') {
        try { command = JSON.parse(typeof options.body === 'string' ? options.body : new TextDecoder().decode(options.body)).method; } catch { /* Учитывается внешняя команда. */ }
      }
      if (!command.startsWith('workspace_probe_')) count(stats.rpc, command);
    }
    return nativeFetch(url, options);
  };
  window.__researchStats = () => JSON.parse(JSON.stringify(stats));
})();
