import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Стенд дизайна графа 2.1.4: настоящее окно (App → Workspace → Sidebar → MiniGraph), а вместо продуктовой graph/PhysicalGraph — панель стенда.
// Подмена только в этой конфигурации; продуктовые файлы и обычная сборка не меняются.
const standGraph = fileURLToPath(new URL("./src/spikes/graph-design/PhysicalGraph.svelte", import.meta.url));

const swapPhysicalGraph: Plugin = {
  name: "bars-graph-design-swap",
  enforce: "pre",
  resolveId(source, importer) {
    if (importer && /(^|[\\/])graph[\\/]PhysicalGraph\.svelte$/.test(source) && /[\\/]src[\\/]lib[\\/]/.test(importer.replaceAll("\\", "/"))) return standGraph;
    return null;
  },
};

export default defineConfig({
  plugins: [swapPhysicalGraph, svelte()],
  clearScreen: false,
  server: { port: 4181, strictPort: true },
  build: { outDir: "graph-design-dist", rollupOptions: { input: "graph-design.html" }, target: "es2022" },
});
