import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Стенд дизайна работы с графом 2.3.1: настоящее окно, а вместо продуктовой graph/PhysicalGraph — панель стенда.
// Подмена только в этой конфигурации; продуктовые файлы и обычная сборка не меняются.
const standGraph = fileURLToPath(new URL("./src/spikes/graph-actions/PhysicalGraph.svelte", import.meta.url));

const swapGraph: Plugin = {
  name: "bars-graph-actions-swap",
  enforce: "pre",
  resolveId(source, importer) {
    if (importer && /(^|[\/])graph[\/]PhysicalGraph\.svelte$/.test(source) && /[\/]src[\/]lib[\/]/.test(importer.replaceAll("\\", "/"))) return standGraph;
    return null;
  },
};

export default defineConfig({
  plugins: [swapGraph, svelte()],
  clearScreen: false,
  server: { port: 4184, strictPort: true },
  build: { outDir: "graph-actions-dist", rollupOptions: { input: "graph-actions.html" }, target: "es2022" },
});
