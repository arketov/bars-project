import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Окно открывается из встроенных файлов `ui/dist` (crates/bars-app/tauri.conf.json), внешних загрузок нет.
export default defineConfig(({ mode }) => ({
  plugins: [svelte()],
  clearScreen: false,
  // Компонентные тесты jsdom используют тот же браузерный runtime Svelte, что и окно.
  ...(mode === "test" ? { resolve: { conditions: ["browser"] } } : {}),
  server: { port: 1420, strictPort: true },
  build: { target: "es2022", outDir: "dist", emptyOutDir: true },
  test: { environment: "jsdom", include: ["src/**/*.test.ts"], setupFiles: ["src/test-setup.ts"] },
}));
