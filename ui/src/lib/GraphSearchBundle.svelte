<script lang="ts">
    import { label as uiLabel, limitText, graphNote } from "./labels";
    import { app } from "./app.svelte";
    import {
        sameRef,
        type GraphReply,
        type GraphEdge,
        type Evidence,
    } from "./graph";
    let { graph, related = false, embedded = false, selected = null, onselect, states = {} }: { graph: GraphReply; related?: boolean; embedded?: boolean; selected?: number | null; onselect?: (index: number) => void; states?: Record<string, string> } =
        $props();
    const nodes = $derived(new Map(graph.nodes.map(node => [node.reference.id, node])));
    const edges = $derived(new Map(graph.assertions.map(edge => [edge.id, edge])));
    const proofs = $derived(new Map(graph.evidence.map(proof => [proof.id, proof])));
    const sources = $derived(new Map(graph.sources.map(source => [source.reference.id, source])));
    const label = (id: string) => {
        const node = nodes.get(id);
        const source = sources.get(id) ?? node?.source_ids?.map(id => sources.get(id)).find(Boolean);
        return node?.reference.kind === "document" ? source?.docs_path ?? node?.label ?? "Документ" : node?.label ?? "Объект недоступен";
    };
    const cards = $derived(graph.items);
    const warnings = $derived(Object.entries(graph.layer_states ?? {}).filter(([,state]) => state !== "ready"));
    function itemProofs(item: GraphReply["items"][number]): Evidence[] {
        const ids = item.type === "path" || item.type === "impact" ? item.path.evidence
            : item.type === "role_group" || item.type === "audit" || item.type === "conflict" ? item.evidence
            : item.type === "node" ? item.node.evidence
            : item.type === "topic" ? item.representatives.flatMap(ref => nodes.get(ref.id)?.evidence ?? []) : [];
        const shown = new Set(assertions(item).flatMap(edge => edge.evidence));
        return [...new Set(ids)].filter(id => !shown.has(id)).map(id => proofs.get(id)).filter((proof): proof is Evidence => !!proof);
    }
    function assertions(item: GraphReply["items"][number]): GraphEdge[] {
        if (item.type === "path" || item.type === "impact")
            return item.path.steps.map(step => edges.get(step.edge)).filter((edge): edge is GraphEdge => !!edge);
        if (item.type === "relation") return [item.assertion];
        if (item.type === "edge") return [item.edge];
        if (item.type === "conflict") return item.assertions.map(id => edges.get(id)).filter((edge): edge is GraphEdge => !!edge);
        return [];
    }
    async function open(proof: Evidence) {
        const candidate = sources.get(proof.document.id);
        const source = candidate && sameRef(candidate.reference, proof.document) ? candidate : undefined;
        if (source && states[source.path] !== "deleted_since_search" && states[source.path] !== "unavailable")
            await app.openDocument({
                path: source.path,
                title: source.title,
                first_line: states[source.path] === "changed_since_search" ? 0 : proof.span.first_line,
                last_line: states[source.path] === "changed_since_search" ? 0 : proof.span.last_line,
                expectedRevision: proof.document.revision,
            });
    }
</script>

<section class="graph-search-bundle">
    <h2>{related ? "Связанные источники" : "Отношения и пути"}</h2>
    <p title={`${graph.used_bytes} байт`}>
        Найдено связанных источников: {graph.sources?.length ?? 0}; элементов: {cards.length}
    </p>
    {#each warnings as [layer,state]}<p class="origin" title={`${layer}: ${state}`}>{layer === "semantic" ? "Смысловой слой" : layer === "similarity" ? "Близость" : "Слой ссылок"}: {uiLabel("layerState", state).text}</p>{/each}
    {#if graph.missing_roles?.length}<p>Не найдены: {graph.missing_roles.map(role => uiLabel("role",role).text).join(", ")}</p>{/if}
    {#each graph.warnings ?? [] as warning}{@const note = graphNote(warning)}<p class="origin" title={note.hint}>{note.text}</p>{/each}
    {#if cards.length === 0}<p>В выбранной области и пределах ничего не найдено. Отсутствие связей не доказано; попробуйте обычный поиск или поиск в тексте.</p>{/if}
    {#if graph.truncated}<p title={graph.limit_reasons.join(", ")}>Показано не всё: {limitText(graph.limit_reasons)}</p>{/if}
    {#each (embedded ? cards : cards.slice(0, 3)) as item, index}
        <article>
            {#if ["path", "impact", "relation", "edge"].includes(item.type)}
            <button class="link" aria-pressed={selected === index} onclick={() => {
                if (embedded) onselect?.(index);
                else { app.graphPackage = index; app.graphShowQuery = true; app.view = "graph"; }
            }}>{embedded ? "Выделить путь на карте" : "Показать путь в графе"}</button>
            {#if embedded}<p>На карте выделяются существующие физические связи. Все шаги и основания — ниже.</p>{/if}
            {/if}
            {#if item.type === "role_group"}<h3>{uiLabel("role", item.role).text}</h3>{/if}
            {#if item.type === "node"}<h3>{label(item.node.reference.id)}</h3>{/if}
            {#if item.type === "topic"}<h3>{item.label}</h3><p>{item.members.length} участников; представители: {item.representatives.map(ref => label(ref.id)).join(", ")}</p>{/if}
            {#if item.type === "audit"}<h3>{item.node ? label(item.node.id) : "Наблюдение"}</h3><p>{item.detail}</p>{/if}
            {#if item.type === "conflict"}<h3>Кандидат в противоречие, не вердикт</h3>{/if}
            {#if item.type === "impact"}<p>{item.direct ? "Прямое влияние" : "Косвенное влияние"}: {label(item.node.id)} — проверьте описание.</p>{/if}
            {#if related}<p class="origin">Источник включён через связи графа; собственной оценки запроса у него нет.</p>{/if}
            {#if item.type === "path" || item.type === "impact"}
                <h3>
                    {item.path.nodes.map((node) => label(node.id)).join(" → ")}
                </h3>
                <p>
                    {item.path.complete
                        ? "Полная цепочка в полученном ответе"
                        : "Неполная цепочка"}
                </p>
                {#if item.path.steps.some((step) => step.reversed)}<p>
                        Есть переходы против направления факта; стрелки ниже
                        показывают направление утверждений.
                    </p>{/if}
            {/if}
            {#each assertions(item) as edge}
                <p class="assertion">
                    {label(edge.from)} → {edge.polarity === "negative"
                        ? "НЕ "
                        : ""}<span title={edge.predicate}>{uiLabel("predicate", edge.predicate).text}</span> → {label(edge.to)}
                </p>
                <p class="origin">
                    {edge.origin === "similarity"
                        ? "Близость по смыслу; смысловое отношение не установлено"
                        : edge.origin === "structural"
                          ? "Структурная связь документации"
                          : "Явное правило извлечения; проверьте основание"}
                </p>
                {#each Object.entries(edge.qualifiers) as [key, value]}<details><summary>Условие связи</summary><p>{key}: {value}</p></details>{/each}
                <details>
                    <summary>Основания перехода: {edge.evidence.length}</summary
                    >
                    {#each edge.evidence as id}
                        {@const proof = proofs.get(id)}
                        {#if proof}
                            {@const source = sources.get(proof.document.id)}
                            <p>
                                {source?.docs_path} · {proof.span
                                    .first_line}–{proof.span.last_line}
                            </p>
                            <pre>{proof.quote}</pre>
                            <button
                                class="link"
                                disabled={!source || ["deleted_since_search", "unavailable"].includes(states[source.path])}
                                onclick={() => void open(proof)}
                                >Открыть текущий файл ↗</button
                            >
                        {/if}
                    {/each}
                </details>
            {/each}
            {#each itemProofs(item) as proof}
                {@const source = sources.get(proof.document.id)}
                <p>{source?.docs_path ?? "Источник недоступен"} · {proof.span.first_line}–{proof.span.last_line}</p>
                <pre>{proof.quote}</pre>
                <button class="link" disabled={!source || ["deleted_since_search", "unavailable"].includes(states[source.path])} onclick={() => void open(proof)}>Открыть текущий файл ↗</button>
            {/each}
        </article>
    {/each}
    {#if !embedded && cards.length > 3}<p>
            Показаны первые 3 пакета из {cards.length}; остальные доступны в
            графе.
        </p>{/if}
    {#if !embedded}<button
        class="ghost"
        onclick={() => {
            app.graphPackage = null;
            app.graphShowQuery = true;
            app.view = "graph";
        }}>Открыть связи и основания в графе ↗</button
    >{/if}
</section>

<style>
    section {
        margin: 14px 0;
        padding: 16px;
        border: 1px solid var(--line);
        border-radius: 12px;
        background: var(--panel);
    }
    h2 {
        font-size: 16px;
        margin: 0 0 8px;
    }
    h3,
    p {
        overflow-wrap: anywhere;
    }
    h3 {
        font-size: 14px;
    }
    p,
    summary {
        font-size: 12px;
    }
    article {
        border-top: 1px solid var(--line);
        padding: 8px 0;
    }
    .origin {
        color: var(--muted);
    }
    pre {
        max-height: 220px;
        overflow: auto;
        white-space: pre-wrap;
        overflow-wrap: anywhere;
        font-size: 12px;
    }
    details {
        margin: 10px 0;
    }
    button {
        margin: 6px 0;
    }
</style>
