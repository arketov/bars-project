import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const directory = path.dirname(fileURLToPath(import.meta.url));
const read = async name => JSON.parse((await readFile(path.join(directory, name), 'utf8')).replace(/^\uFEFF/, ''));
const deltaMap = (before, after) => Object.fromEntries(Object.keys(after).map(key => [key, after[key] - (before[key] ?? 0)]));
const median = values => { const sorted = [...values].sort((a, b) => a - b); return (sorted[Math.floor((sorted.length - 1) / 2)] + sorted[Math.floor(sorted.length / 2)]) / 2; };
const reports = {};
for (const name of ['installed', 'isolated']) {
  const report = await read(`${name}.json`);
  if (!report.success) throw Error(`${name}: incomplete`);
  const samples = report.samples.map(sample => {
    const seconds = (sample.after.time - sample.before.time) / 1000;
    const roles = {};
    for (const row of sample.after.processes) {
      const old = sample.before.processes.find(value => value.pid === row.pid && value.start_utc === row.start_utc);
      const role = roles[row.role] ??= { cpu_seconds: 0, cpu_percent_core: 0, logical_read_bytes: 0, private_bytes: 0, working_bytes: 0 };
      if (old) { role.cpu_seconds += row.cpu - old.cpu; role.cpu_percent_core += 100 * (row.cpu - old.cpu) / seconds; role.logical_read_bytes += row.logical_read - old.logical_read; }
      role.private_bytes += row.private; role.working_bytes += row.working;
    }
    const same = (a, b) => a.pid === b.pid && a.start_utc === b.start_utc;
    const valid = [sample.before, sample.after].every(point => point.windows.some(window => window.Title.startsWith('bars') && window.Visible === sample.visible && (!sample.visible || !window.Minimized)));
    const result = { round: sample.round, visible: sample.visible, instrumented: sample.instrumented ?? false, valid, seconds, roles,
      new_processes: sample.after.processes.filter(row => !sample.before.processes.some(old => same(row, old))).map(row => row.pid),
      exited_processes: sample.before.processes.filter(row => !sample.after.processes.some(after => same(row, after))).map(row => row.pid),
      compilers: [...sample.before.compilers, ...sample.after.compilers] };
    if (sample.start?.stats) {
      const before = sample.start.stats, after = sample.end.stats;
      result.dom_hidden = { before: sample.start.hidden, after: sample.end.hidden };
      result.frames = after.frames - before.frames;
      result.rpc = deltaMap(before.rpc, after.rpc);
      result.worker_sent = deltaMap(before.workers.sent, after.workers.sent);
      result.worker_received = deltaMap(before.workers.received, after.workers.received);
      result.timers = after.timers.map((timer, index) => ({ ...timer, fires: timer.fires - (before.timers[index]?.fires ?? 0) })).filter(timer => timer.fires);
    }
    if (sample.performanceBefore) {
      const before = Object.fromEntries(sample.performanceBefore.metrics.map(row => [row.name, row.value]));
      result.cdp_durations = Object.fromEntries(sample.performanceAfter.metrics.filter(row => /^(TaskDuration|ScriptDuration|LayoutDuration|RecalcStyleDuration)$/.test(row.name)).map(row => [row.name, row.value - before[row.name]]));
    }
    return result;
  });
  const groups = [];
  for (const instrumented of [...new Set(samples.map(row => row.instrumented))]) {
    for (const visible of [false, true]) {
      const selected = samples.filter(row => row.valid && row.instrumented === instrumented && row.visible === visible);
      if (!selected.length) continue;
      groups.push({ instrumented, visible, count: selected.length, cpu_percent_core_median: Object.fromEntries(Object.keys(selected[0].roles).map(role => [role, median(selected.map(row => row.roles[role]?.cpu_percent_core ?? 0))])) });
    }
  }
  const windowState = point => point?.windows.find(window => window.Title === 'bars');
  const initial = windowState(report.initial), final = windowState(report.final);
  reports[name] = { groups, samples, excluded: samples.filter(row => !row.valid).map(row => ({ round: row.round, visible: row.visible, reason: 'HWND visibility/minimized mismatch at boundary' })),
    restoration: initial && final ? { visible: initial.Visible === final.Visible, minimized: initial.Minimized === final.Minimized } : null, fault_injection: report.fault_injection };
}
await writeFile(path.join(directory, 'summary.json'), JSON.stringify(reports, null, 2));
for (const [name, report] of Object.entries(reports)) console.log(name, JSON.stringify(report.groups));
