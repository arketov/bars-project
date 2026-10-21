import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  build: { outDir: "docking-dist", rollupOptions: { input: "docking.html" }, target: "es2022" },
});
