import "../graph-design/theme-param";
import { mount } from "svelte";
import App from "../../App.svelte";
import { app } from "../../lib/app.svelte";
import "../../theme.css";
import "../../states.css";
import { standSlice } from "./physical";

// Только стенд раунда 27. Параметры адреса: data=<имя выгрузки>, theme=light|dark,
// focus=<часть пути документа> — центр локального графа мини-панели, view=graph — открыть вкладку «Граф».
const params = new URLSearchParams(location.search);
mount(App, { target: document.getElementById("app")! });

void standSlice().then((slice) => {
  const focus = params.get("focus");
  if (focus) app.activeDocument = slice.nodes.find((n) => n.kind === "document" && n.path.includes(focus))?.path ?? null;
  if (params.get("view") === "graph") setTimeout(() => (app.view = "graph"), 300);
});
