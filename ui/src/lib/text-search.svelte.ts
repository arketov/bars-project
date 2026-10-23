// Т-39: независимая выдача по диску; поздние ответы и порции не меняют новую сессию.
import { call } from './api';
export interface TextHit { line: number; column: number; length: number; text: string; ranges: [number, number][] }
export interface TextDocument { path: string; module: string; title: string; revision: string; hits_total: number; hits: TextHit[]; has_more: boolean }
export interface TextOutput { documents: TextDocument[]; docs_total: number; hits_total: number; truncated: string | null; skipped: {path:string;reason:string}[]; elapsed_ms:number }
export interface TextQuery { project:string; query:string; modules:string[]; files:string[]; case_sensitive:boolean; regex:boolean; path_filter:string; request_id:string }

export class TextSearchStore {
  caseSensitive = $state(false);
  regex = $state(false);
  pathFilter = $state('');
  output = $state<TextOutput | null>(null);
  error = $state('');
  loading = $state(false);
  more = $state<string[]>([]);
  #generation = 0;
  #params: TextQuery | null = null;

  reset() { this.#generation++; this.output = null; this.error = ''; this.loading = false; this.more=[]; this.#params=null; }
  async search(params: TextQuery) {
    this.reset();
    const generation = this.#generation;
    this.#params = params;
    this.loading = true;
    try {
      const output = await call<TextOutput>('text_search', {...params});
      if (generation === this.#generation) this.output = output;
    } catch (e) { if (generation === this.#generation) this.error = e instanceof Error ? e.message : String(e); }
    finally { if (generation === this.#generation) this.loading = false; }
  }
  async loadMore(path: string) {
    const doc = this.output?.documents.find(d => d.path === path), params=this.#params;
    if (!doc || !params || this.more.length) return;
    const generation = this.#generation;
    this.more = [path]; this.error='';
    try {
      const output = await call<TextOutput>('text_search', {...params, path, offset:doc.hits.length, revision:doc.revision});
      if (generation !== this.#generation) return;
      const next = output.documents[0];
      if (!next) {
        if (output.skipped.length) this.error=output.skipped.map(s => `${s.path}: ${s.reason}`).join('; ');
        else { doc.hits=[]; doc.hits_total=0; doc.has_more=false; this.error='Вхождения изменились на диске. Повторите поиск.'; }
        return;
      }
      const changed = next.revision !== doc.revision;
      if (changed) { doc.hits=next.hits; doc.revision=next.revision; }
      else doc.hits.push(...next.hits);
      doc.hits_total=changed ? next.hits_total : Math.max(doc.hits_total,next.hits_total); doc.has_more=next.has_more;
      if (this.output) {
        this.output.hits_total=this.output.documents.reduce((sum,d)=>sum+d.hits_total,0);
        if (changed && output.truncated) this.output.truncated=output.truncated;
      }
      if (output.skipped.length) this.error=output.skipped.map(s => `${s.path}: ${s.reason}`).join('; ');
    } catch(e) { if (generation === this.#generation) this.error=e instanceof Error ? e.message : String(e); }
    finally { if (generation === this.#generation) this.more=[]; }
  }
}
