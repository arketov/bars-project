import "./theme-param";
import { mount } from "svelte";
import App from "../../App.svelte";
import "../../theme.css";
import "../../states.css";

// Только стенд дизайна 2.1.4. App вне Tauri использует штатный mock.ts; граф подменяет vite.graph-design.config.ts.
// Параметры адреса: theme=light|dark, scenario=…, mode=links|meaning|chunks|all, scale=project, query=<текст>, motion=reduced, smin=0.82,
// expand=<часть названия>, select=<часть названия>, edge=mutual|markdown_link|document_similarity, zoom=<масштаб>.
mount(App, { target: document.getElementById("app")! });
