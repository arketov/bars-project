import { app } from "./app.svelte";
import { entryCreateDoc } from "./api";
import { documentTargets, resolveWikilink, wikiTarget } from "./link-picker";
import { resolveDocumentLink } from "./markdown";
import { docsPrefix, projectPathOf } from "./tree";

/** Только путь внутри docs; имена проверит общий обработчик создания файла. */
export function missingLinkPath(source: string, docsRoot: string, raw: string, wiki: boolean): string | null {
  const name = wiki ? wikiTarget(raw)?.replace(/\\/g, "/") : null;
  const path = wiki && name ? name.includes("/") ? projectPathOf(`${name}.md`, docsRoot) : [...source.split("/").slice(0,-1), `${name}.md`].join("/")
    : !wiki ? resolveDocumentLink(source, raw) : null;
  if (!path) return null;
  const root = docsPrefix(docsRoot);
  if (root && !path.startsWith(root + "/")) return null;
  if (path.startsWith("/") || path.split("/").some(part => !part || part === "." || part === "..") || /[:*?"<>|\x00-\x1f]/.test(path)) return null;
  return path;
}

class FollowLink {
  pending = $state<{project: string; path: string} | null>(null);
  error = $state(""); busy = $state(false);
  async open(project: string, source: string, raw: string, wiki: boolean): Promise<void> {
    const targets = documentTargets(app.tree, app.docsRoot, true);
    const target = wiki ? resolveWikilink(targets, source, raw)?.path : resolveDocumentLink(source, raw);
    if (target && targets.some(t => t.path === target)) {
      await app.openDocument({path: target, title: target.split("/").pop() ?? target, first_line:0,last_line:0}); return;
    }
    const path = missingLinkPath(source, app.docsRoot, raw, wiki);
    if (!path) { app.flash("Недопустимый путь ссылки", true); return; }
    this.pending = {project,path}; this.error = "";
  }
  async create(): Promise<void> {
    const request = this.pending; if (!request || this.busy) return;
    if (app.current && app.current !== request.project) { this.error = "Проект изменился. Откройте ссылку заново."; return; }
    this.busy = true; this.error = "";
    try {
      const slash = request.path.lastIndexOf("/");
      const result = await entryCreateDoc(request.project, slash < 0 ? "." : request.path.slice(0,slash), request.path.slice(slash+1));
      if (result.status !== "ok") { this.error = result.message ?? "Не удалось создать документ"; return; }
      this.pending = null;
      if (app.current && app.current !== request.project) return;
      await app.loadTree();
      await app.openDocument({path:result.path,title:result.path.split("/").pop() ?? result.path,first_line:0,last_line:0});
    } catch (error) { this.error = String(error); }
    finally { this.busy = false; }
  }
}
export const followLink = new FollowLink();
