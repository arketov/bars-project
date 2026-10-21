// Полный публичный API каждого кандидата, одинаковый Vite minify; не размер интеграции bars.
import { build } from "vite";
import { execFileSync } from "node:child_process";
import { mkdtemp, writeFile, readFile, readdir, rm, realpath } from "node:fs/promises";
import { join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { gzipSync } from "node:zlib";

const root = await mkdtemp(join(tmpdir(), "bars-docking-bundle-"));
const target = resolve(process.argv[2] ?? "../docs/chats/2026-10-20-window-docking/bundles.json");
const npmCli = process.env.npm_execpath;
if (!npmCli) throw new Error("Запускайте через npm run probe:docking:measure");
const packages = ["dockview-core@8.6.1", "dockview@8.6.1", "golden-layout@2.6.0", "dock-spawn-ts@4.4.0"];
try {
  execFileSync(process.execPath, [npmCli, "install", "--prefix", root, "--no-audit", "--no-fund", "--ignore-scripts", "--no-package-lock", ...packages], { stdio: "pipe" });
  const rows = [];
  for (const name of ["dockview-core", "golden-layout", "dock-spawn-ts"]) {
    const path = join(root, "node_modules", name);
    const pkg = JSON.parse(await readFile(join(path, "package.json"), "utf8"));
    const entry = join(root, `${name}.mjs`);
    await writeFile(entry, `import * as api from ${JSON.stringify(join(path, pkg.module ?? pkg.main).replaceAll("\\", "/"))}; globalThis.dockingProbe = api;`);
    const out = join(root, `out-${name}`);
    const metadata = JSON.parse(execFileSync(process.execPath, [npmCli, "view", `${name}@${pkg.version}`, "version", "license", "dist.unpackedSize", "time.modified", "repository.url", "dependencies", "--json"], { encoding: "utf8" }));
    try {
      await build({ configFile: false, logLevel: "silent", build: { outDir: out, minify: true, lib: { entry, formats: ["es"], fileName: "api" } } });
      const files = await readdir(out);
      const js = await readFile(join(out, files.find(file => file.endsWith(".mjs") || file.endsWith(".js"))));
      rows.push({ name, ...metadata, jsBytes: js.length, jsGzipBytes: gzipSync(js).length });
    } catch (error) {
      rows.push({ name, ...metadata, buildError: String(error).replaceAll(root, "<temporary>").replaceAll(root.replaceAll("\\", "/"), "<temporary>").replace(/bars-docking-bundle-[A-Za-z0-9_-]+/g, "bars-docking-bundle-<temporary>").replace(/\x1b\[[0-9;]*m/g, "").slice(0, 2400) });
    }
  }
  const css = await readFile(join(root, "node_modules/dockview/dist/styles/dockview.css"));
  await writeFile(target, JSON.stringify({ measuredAt: new Date().toISOString(), method: "Vite 8.3.4, full namespace export retained, es minify; gzip Node default; CSS dockview raw", rows, dockviewCss: { bytes: css.length, gzipBytes: gzipSync(css).length } }, null, 2) + "\n");
  console.log(await readFile(target, "utf8"));
} finally {
  const checked = await realpath(root);
  if (!checked.startsWith(await realpath(tmpdir())) || !checked.includes("bars-docking-bundle-")) throw new Error("Неверный временный каталог");
  await rm(checked, { recursive: true, force: true });
}
