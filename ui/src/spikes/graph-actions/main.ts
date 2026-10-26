import "../graph-design/theme-param";
import { mount } from "svelte";
import "../../theme.css";
import "../../states.css";

// Стенд дизайна 2.3.4 (docs/features/graphrag/graph-actions-design.md). App вне Tauri использует штатный mock.ts; граф подменяет vite.graph-actions.config.ts.
// Параметры адреса: theme, level=documents|chunks|entities|mixed, mode=links|meaning|all, select=<часть названия>, view=local|path|impact|context|topics|audit,
// to=<вторая вершина>, sem=semantic|mixed, depth=1|2|3, sub=<части названий через запятую>, pop=filters|analysis|predicates,
// ctx=1, query=<текст>, scenario=<состояние GR-05>, motion=reduced.
// panel=1 isolates the same graph component for responsive design checks.
const Component = new URLSearchParams(location.search).has('panel')
  ? (await import('./PhysicalGraph.svelte')).default
  : (await import('../../App.svelte')).default;
const standTheme = new URLSearchParams(location.search).get('theme');
if (standTheme === 'light' || standTheme === 'dark') document.documentElement.dataset.mode = standTheme;
document.getElementById('app')!.style.height = '100vh';
if (new URLSearchParams(location.search).get('page') === 'graph') {
  const { app } = await import('../../lib/app.svelte');
  app.view = 'graph';
}
mount(Component, { target: document.getElementById("app")! });
