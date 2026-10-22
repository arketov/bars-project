import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Стенд раунда 27 (граф как в Obsidian): настоящее окно с продуктовым графом, а срез `graph_physical` — выгрузка
// настоящего индекса проекта. Выгрузки не коммитятся: папка `GRAPH_SLICES` или `ui/.graph-slices/` (в .gitignore),
// файлы `<имя>.json`, адрес `?data=<имя>`. Подменяется только заглушка среза; продуктовые файлы и обычная сборка не меняются.
const standSlice = fileURLToPath(new URL("./src/spikes/graph-stand/physical.ts", import.meta.url));
const slices = process.env.GRAPH_SLICES ?? fileURLToPath(new URL("./.graph-slices/", import.meta.url));

const stand: Plugin = {
  name: "bars-graph-stand",
  enforce: "pre",
  resolveId(source, importer) {
    if (source === "./mock-physical" && importer && /[\\/]src[\\/]lib[\\/]mock\.ts$/.test(importer)) return standSlice;
    return null;
  },
  configureServer(server) {
    server.middlewares.use("/__slices/", (req, res) => {
      const name = (req.url ?? "").replace(/^\//, "").replace(/\.json.*$/, "");
      const file = join(slices, `${name}.json`);
      if (!/^[\w-]+$/.test(name) || !existsSync(file)) {
        res.statusCode = 404;
        res.end("нет выгрузки");
        return;
      }
      res.setHeader("content-type", "application/json; charset=utf-8");
      res.end(readFileSync(file));
    });
  },
};

export default defineConfig({
  plugins: [stand, svelte()],
  clearScreen: false,
  server: { port: 4183, strictPort: true },
  build: { outDir: "graph-stand-dist", rollupOptions: { input: "graph-stand.html" }, target: "es2022" },
});
