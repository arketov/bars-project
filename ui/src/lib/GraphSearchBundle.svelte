<script lang="ts">
    import { app } from "./app.svelte";
    import {
        sameRef,
        type GraphReply,
        type GraphEdge,
        type Evidence,
    } from "./graph";
    let { graph, related = false }: { graph: GraphReply; related?: boolean } =
        $props();
    const label = (id: string) =>
        graph.nodes.find((node) => node.reference.id === id)?.label ?? id;
    const cards = $derived(
        graph.items.filter((item) =>
            ["path", "impact", "relation", "edge"].includes(item.type),
        ),
    );
    function assertions(item: GraphReply["items"][number]): GraphEdge[] {
        if (item.type === "path" || item.type === "impact")
            return item.path.steps.flatMap((step) =>
                graph.assertions.filter((edge) => edge.id === step.edge),
            );
        if (item.type === "relation") return [item.assertion];
        if (item.type === "edge") return [item.edge];
        return [];
    }
    async function open(proof: Evidence) {
        const source = graph.sources.find((source) =>
            sameRef(source.reference, proof.document),
        );
        if (source)
            await app.openDocument({
                path: source.path,
                title: source.title,
                first_line: proof.span.first_line,
                last_line: proof.span.last_line,
            });
    }
</script>

<section class="graph-search-bundle">
    <h2>{related ? "Связанные источники" : "Отношения и пути"}</h2>
    <p>
        {graph.items.length} элементов · {graph.used_bytes} байт · смысловой слой
        частичный
    </p>
    {#if graph.truncated}<p>Усечено: {graph.limit_reasons.join(", ")}</p>{/if}
    {#each cards.slice(0, 3) as item}
        <article>
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
                        : ""}{edge.predicate} → {label(edge.to)}
                </p>
                <p class="origin">
                    {edge.origin === "similarity"
                        ? "Близость embeddings; смысловое отношение не установлено"
                        : edge.origin === "structural"
                          ? "Структурная связь документации"
                          : "Явное правило извлечения; проверьте основание"}
                </p>
                {#each Object.entries(edge.qualifiers) as [key, value]}<p>
                        {key}: {value}
                    </p>{/each}
                <details>
                    <summary>Основания перехода: {edge.evidence.length}</summary
                    >
                    {#each edge.evidence as id}
                        {@const proof = graph.evidence.find(
                            (proof) => proof.id === id,
                        )}
                        {#if proof}
                            {@const source = graph.sources.find((source) =>
                                sameRef(source.reference, proof.document),
                            )}
                            <p>
                                {source?.docs_path} · {proof.span
                                    .first_line}–{proof.span.last_line}
                            </p>
                            <pre>{proof.quote}</pre>
                            <button
                                class="link"
                                onclick={() => void open(proof)}
                                >Открыть текущий файл ↗</button
                            >
                        {/if}
                    {/each}
                </details>
            {/each}
        </article>
    {/each}
    {#if cards.length > 3}<p>
            Показаны первые 3 пакета из {cards.length}; остальные доступны в
            графе.
        </p>{/if}
    <button
        class="ghost"
        onclick={() => {
            app.graphPreview = graph;
            app.view = "graph";
        }}>Открыть связи и основания в графе ↗</button
    >
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
