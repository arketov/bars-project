import "../graph-design/theme-param";
import { mount } from "svelte";
import App from "../../App.svelte";
import "../../theme.css";
import "../../states.css";

// Только стенд дизайна 2.3.1 (docs/features/graphrag/graph-actions-design.md). App вне Tauri использует штатный mock.ts; граф подменяет vite.graph-actions.config.ts.
// Параметры адреса: theme, level=documents|chunks|entities|mixed, mode=links|meaning|all, select=<часть названия>, view=local|path|impact|context|topics|audit,
// to=<вторая вершина>, sem=semantic|mixed, depth=1|2|3, sub=<части названий через запятую>, list=nodes|edges|view, pop=filters|analysis|predicates,
// ctx=1, query=<текст>, scenario=<состояние GR-05>, motion=reduced.
mount(App, { target: document.getElementById("app")! });
