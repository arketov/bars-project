import { defineConfig } from "vite";

export default defineConfig({
  root: ".",
  server: { port: 4182, strictPort: true, open: false },
  build: { outDir: "editor-dist", rollupOptions: { input: "editor.html" }, target: "es2022" },
});
