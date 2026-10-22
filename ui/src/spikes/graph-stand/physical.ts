// Только стенд раунда 27: срез `graph_physical` из выгрузки настоящего индекса (`?data=<имя>`), вместо синтетической заглушки.
import type { ServerSlice } from "../../lib/graph/slice";
import { graphFixture } from "../../lib/mock-graph";

const name = new URLSearchParams(location.search).get("data") ?? "bars";
let cache: Promise<ServerSlice> | null = null;

export function standSlice(): Promise<ServerSlice> {
  cache ??= fetch(`/__slices/${name}.json`).then((r) => {
    if (!r.ok) throw new Error(`Нет выгрузки среза «${name}»`);
    return r.json() as Promise<ServerSlice>;
  });
  return cache;
}

export async function physicalFixture(_params: unknown): Promise<ServerSlice> {
  const slice = structuredClone(await standSlice());
  // Снимок среза совпадает со снимком состояния графа заглушки: иначе окно каждые 2 с считает граф изменённым и перезагружает срез.
  const status = (await graphFixture("graph_status", {})) as { snapshot?: string };
  if (status.snapshot) slice.snapshot = status.snapshot;
  return slice;
}
