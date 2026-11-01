import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Стенд использует продуктовые компоненты 3.4.8; подменяются только ответы поиска.
export default defineConfig({
  plugins: [{
    name: "bars-query-card-design",
    enforce: "pre",
    transform(source, id) {
      const path = id.replaceAll("\\", "/");
      if (path.endsWith("/src/lib/mock.ts")) {
        const original = "if (method === 'text_search') return {documents:[],docs_total:0,hits_total:0,truncated:null,skipped:[],elapsed_ms:0};";
        if (source.split(original).length !== 2) throw new Error("Макет карточки: изменился mock text_search");
        return 'import { queryCardText } from "../spikes/query-card/text-fixture";\n' + source.replace(original, "if (method === 'text_search') return queryCardText();");
      }
      return null;
    },
  }, svelte()],
  server: { host: "127.0.0.1", port: 4187, strictPort: true },
  build: { outDir: "query-card-design-dist", rollupOptions: { input: "query-card-design.html" } },
});
