import { afterEach, describe, expect, it, vi } from 'vitest';
import { jobActions } from './job';
import corpus from './overdrive.json';

describe('макет входа в граф', () => {
  afterEach(() => { vi.unstubAllGlobals(); vi.resetModules(); });
  it('строит второй корпус тем же компонентом данных без вымышленных смысловых связей', async () => {
    vi.stubGlobal('location', { search: '?project=overdrive-bots' });
    vi.resetModules();
    const { F, XStore, buildLevel } = await import('./data');
    const store = new XStore();
    const slice = buildLevel(store, { level: 'documents', expanded: {}, scenario: '', chunkLimit: 500, showUnresolved: false, topics: null });
    expect(F.name).toContain('overdrive-bots');
    expect(slice.visible).toHaveLength(corpus.docs.length);
    expect(slice.edges).toHaveLength(corpus.links.length);
    expect(slice.edges.every(e => e.kind === 'markdown_link')).toBe(true);
    expect(store.nodes.filter(n => n.kind === 'chunk')).toHaveLength(0);
  });
  it('не разрешает повторный запуск во время работы и запроса остановки', () => {
    expect(jobActions('idle')).toEqual({ update: false, stop: false });
    expect(jobActions('idle', true)).toEqual({ update: true, stop: false });
    expect(jobActions('running')).toEqual({ update: false, stop: true });
    expect(jobActions('stopping')).toEqual({ update: false, stop: false });
    expect(jobActions('stopped')).toEqual({ update: true, stop: false });
    expect(jobActions('failed')).toEqual({ update: true, stop: false });
  });
  it('ссылки второго корпуса указывают на существующие документы и строки', () => {
    expect(corpus.docs.length).toBeGreaterThan(40);
    for (const [from, to, lines] of corpus.links) {
      expect(corpus.docs[from as number]).toBeDefined();
      expect(corpus.docs[to as number]).toBeDefined();
      for (const line of lines as number[]) {
        expect(line).toBeGreaterThan(0);
        expect(corpus.evidence).toHaveProperty(`link-${from}-${to}-${line}`);
      }
    }
  });
  it('не подменяет незагруженную семантику и близость выдуманными связями', () => {
    expect(corpus.semantic_state).toBe('not_built');
    expect(corpus.docSims).toEqual([]);
    expect(corpus.assertions).toEqual([]);
    expect(corpus.provenance.sources.length).toBe(corpus.docs.length);
  });
  it('все сохранённые строки ссылок совпадают с источниками снимка', () => {
    // Проверка внутренней целостности снимка, без доступа к другому репозиторию в CI.
    for (const [from, to, lines] of corpus.links) for (const line of lines as number[]) {
      const ev = corpus.evidence[`link-${from}-${to}-${line}` as keyof typeof corpus.evidence];
      expect(ev[0]).toBe(from);
      expect(ev[1]).toBe(line);
      expect(String(ev[3])).toMatch(/\[[^\]]+\]\([^)]+\)/);
    }
  });
});
