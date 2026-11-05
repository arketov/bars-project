// Временная область поиска из графа («Искать в подграфе», 2.3.7, OQ-141): документы подграфа заменяют область поиска и
// галочки дерева, прежняя область с запросом, выдачей и прокруткой возвращается одной кнопкой. Дизайн —
// docs/features/graphrag/graph-actions-design.md, раздел «Переходы между поиском и графом».
import { app } from "./app.svelte";
import { findDocumentNode } from "./tree";

interface SavedSearch {
  selected: ReadonlySet<string>;
  query: string;
  results: typeof app.results;
  displayed: typeof app.displayed;
  searchedQuery: string;
  scroll: number;
}

export interface TempScope {
  project: string;
  /** Набор галочек, поставленный графом: любое другое значение `app.selected` — ручная правка, она завершает область. */
  expected: ReadonlySet<string>;
  documents: number;
  /** Документы подграфа, которых нет в дереве проекта (удалены или вне проекта). */
  skipped: number;
  previous: SavedSearch;
}

export type ApplyResult = { ok: true; documents: number; skipped: number } | { ok: false; reason: string };

class GraphScope {
  state = $state.raw<TempScope | null>(null);
  /** Растёт при каждом переходе из графа: поле поиска забирает фокус. */
  focusTick = $state(0);
  /** Прокручиваемая область выдачи; задаёт панель поиска, чтобы возврат восстановил прокрутку. */
  scrollHost: HTMLElement | null = null;

  /** Временная область действует, пока галочки дерева те, что поставил граф, и проект тот же. */
  get active(): TempScope | null {
    const s = this.state;
    return s && s.project === app.current && app.selected === s.expected ? s : null;
  }

  /** Заменяет область документами подграфа (пути от корня проекта). Исходная область при повторной замене не перезаписывается. */
  apply(documentPaths: string[]): ApplyResult {
    const project = app.current;
    if (!project) return { ok: false, reason: "Проект не выбран." };
    const nodes = new Set<string>();
    for (const path of new Set(documentPaths)) {
      const node = findDocumentNode(app.tree, path);
      if (node) nodes.add(node.path);
    }
    const skipped = new Set(documentPaths).size - nodes.size;
    if (!nodes.size) return { ok: false, reason: "В подграфе нет документов, которые можно искать: они удалены или находятся вне проекта." };
    const keep = this.active?.previous ?? this.snapshot();
    const expected: ReadonlySet<string> = nodes;
    app.selected = expected;
    this.state = { project, expected, documents: nodes.size, skipped, previous: keep };
    app.view = "main";
    this.focusTick++;
    return { ok: true, documents: nodes.size, skipped };
  }

  /** «Вернуть прежнюю область»: галочки, запрос, выдача и прокрутка возвращаются без нового поиска. */
  restore(): void {
    const s = this.active;
    this.state = null;
    if (!s) return;
    const p = s.previous;
    app.selected = p.selected;
    app.query = p.query;
    app.results = p.results;
    app.displayed = p.displayed;
    app.searchedQuery = p.searchedQuery;
    const host = this.scrollHost;
    if (host) requestAnimationFrame(() => { host.scrollTop = p.scroll; });
    this.focusTick++;
  }

  private snapshot(): SavedSearch {
    return {
      selected: app.selected,
      query: app.query,
      results: app.results,
      displayed: app.displayed,
      searchedQuery: app.searchedQuery,
      scroll: this.scrollHost?.scrollTop ?? 0,
    };
  }
}

export const graphScope = new GraphScope();
