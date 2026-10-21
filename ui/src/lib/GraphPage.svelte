<script lang="ts">
    import { onDestroy, untrack } from "svelte";
    import { app } from "./app.svelte";
    import { call, inTauri } from "./api";
    import { GraphRenderer } from "./graph-renderer";
    import Icon from "./Icon.svelte";
    import {
        kindLabels,
        operationLabels,
        roleLabels,
        stateLabels,
        sameRef,
        type GraphReply,
        type GraphNode,
        type GraphEdge,
        type GraphOperation,
        type GraphView,
        type GraphStatus,
        type GraphSource,
        type ObjectRef,
        type Evidence,
    } from "./graph";

    let container: HTMLElement | undefined = $state(),
        renderer: GraphRenderer | null = null;
    let reply: GraphReply | null = $state(null),
        status: GraphStatus | null = $state(null),
        error = $state(""),
        rendererError = $state("");
    let busy = $state(false),
        operation: GraphOperation = $state("graph"),
        view: GraphView = $state("documents");
    let query = $state(app.query),
        to = $state(""),
        from = $state(""),
        selected: GraphNode | null = $state(null),
        edge: GraphEdge | null = $state(null);
    let local = $state(false),
        asList = $state(false),
        grouped = $state(false),
        depth = $state(2),
        analysis = $state("topics"),
        grouping = $state("communities"),
        taskProfile = $state("implementation"),
        whole = $state(true);
    let source: Evidence | null = $state(null),
        sourceDialog: HTMLDialogElement | undefined = $state(),
        highlight: string[] = $state([]),
        pinned = $state(false);
    let sourceMeta: GraphSource | null = $state(null);
    let fromRef: ObjectRef | null = $state(null),
        toRef: ObjectRef | null = $state(null);
    let epoch = 0,
        destroyed = false,
        timer: ReturnType<typeof setInterval> | undefined;
    let lastRequest: Record<string, unknown> | null = null;
    const current = $derived(app.project?.path ?? null),
        scope = $derived(app.scope);
    const labels = (id: string) =>
        reply?.nodes.find((n) => n.reference.id === id)?.label ?? id;
    const proofs = $derived.by(
        () => edge?.evidence ?? selected?.evidence ?? [],
    );

    async function updateStatus(path: string) {
        try {
            const value = await call<GraphStatus>("graph_status", {
                cwd: path,
                project: path,
            });
            if (!destroyed && current === path) {
                status = value;
                if (
                    reply &&
                    value.snapshot &&
                    reply.snapshot !== value.snapshot &&
                    ["ready", "partial"].includes(value.state) &&
                    !busy
                ) {
                    selected = null;
                    edge = null;
                    void run();
                }
            }
        } catch (e) {
            if (!destroyed && current === path) error = String(e);
        }
    }
    async function run(cursor?: string, roots?: ObjectRef[]) {
        if (!current) return;
        const path = current,
            id = ++epoch;
        busy = true;
        error = "";
        highlight = [];
        const name = (label: string) => ({
            label,
            kind: null,
            namespace: null,
        });
        let request: Record<string, unknown>;
        if (cursor && lastRequest)
            request = { ...lastRequest, cursor, snapshot: reply?.snapshot };
        else {
            request = {
                operation,
                scope,
                budgets: { max_depth: depth },
                view,
                limit: 100,
            };
            if (operation === "graph") {
                request.local = local || !!roots;
                request.roots =
                    roots ?? (local && selected ? [selected.reference] : []);
            }
            if (operation === "relations") {
                if (selected) request.entity_ref = selected.reference;
                else request.query = query;
            }
            if (operation === "paths") {
                request.from =
                    fromRef ?? (from ? name(from) : selected?.reference);
                request.to = toRef ?? name(to);
                request.path_kind = "mixed";
            }
            if (operation === "impact")
                request.roots = selected ? [selected.reference] : [];
            if (operation === "context") {
                request.query = query;
                request.token_budget = 4096;
                request.task_profile = taskProfile;
            }
            if (operation === "explore") {
                request.analysis = analysis;
                request.grouping = grouping;
                request.whole_scope = whole;
                if (!whole) request.topic = query;
            }
            lastRequest = request;
        }
        try {
            const result = await call<GraphReply>("graph_query", {
                cwd: path,
                project: path,
                request,
            });
            if (destroyed || id !== epoch || current !== path) return;
            reply = result;
            edge = null;
            if (
                selected &&
                !result.nodes.some((n) =>
                    sameRef(n.reference, selected!.reference),
                )
            )
                selected = null;
        } catch (e) {
            if (!destroyed && id === epoch) {
                error = e instanceof Error ? e.message : String(e);
                reply = null;
                edge = null;
            }
        } finally {
            if (!destroyed && id === epoch) busy = false;
        }
    }
    function pick(id: string, isEdge = false) {
        if (isEdge) {
            edge = reply?.assertions.find((e) => e.id === id) ?? null;
            return;
        }
        selected =
            reply?.nodes.find((n) => n.reference.id === id) ??
            reply?.candidates.find((n) => n.reference.id === id) ??
            null;
        edge = null;
        pinned = selected
            ? (renderer?.pinned(selected.reference.id) ?? false)
            : false;
    }
    async function rebuild() {
        if (!current) return;
        const path = current;
        try {
            if (status?.active_job) {
                await call("graph_cancel", {
                    cwd: path,
                    project: path,
                    job_id: status.active_job,
                });
                app.flash(
                    "Остановка запрошена; окончательный исход появится в состоянии графа",
                );
            } else {
                await call("graph_build", { cwd: path, project: path });
                app.flash("Задание графа принято");
            }
            await updateStatus(path);
        } catch (e) {
            error = String(e);
        }
    }
    function openProof(ev: Evidence) {
        sourceMeta =
            reply?.sources.find((s) => sameRef(s.reference, ev.document)) ??
            null;
        source = ev;
        sourceDialog?.showModal();
    }
    async function openOriginal() {
        if (!source || !sourceMeta) return;
        const evidence = source;
        const meta = sourceMeta;
        sourceDialog?.close();
        await app.openDocument({
            path: meta.path,
            title: meta.title,
            first_line: evidence.span.first_line,
            last_line: evidence.span.last_line,
        });
    }
    function startRenderer(element: HTMLElement, key: string) {
        try {
            renderer = new GraphRenderer(element, key, pick);
            rendererError = "";
        } catch (e) {
            rendererError = "Карта недоступна: " + String(e);
            asList = true;
        }
    }
    $effect(() => {
        const path = current;
        void scope;
        if (!path) return;
        epoch++;
        reply = null;
        selected = null;
        fromRef = null;
        toRef = null;
        edge = null;
        status = null;
        lastRequest = null;
        void updateStatus(path);
        untrack(() => {
            if (app.graphPreview) {
                reply = app.graphPreview;
                app.graphPreview = null;
            } else void run();
        });
        timer = setInterval(() => {
            void updateStatus(path);
        }, 1000);
        return () => {
            if (timer) clearInterval(timer);
        };
    });
    $effect(() => {
        const element = container,
            key = current + ":" + view;
        if (!element || !current) return;
        startRenderer(element, key);
        return () => {
            renderer?.destroy();
            renderer = null;
        };
    });
    $effect(() => {
        const data = reply,
            selection = selected?.reference.id ?? null,
            group = grouped,
            steps = highlight;
        if (data && renderer) {
            try {
                renderer.set(data, selection, group, steps);
            } catch (e) {
                rendererError = String(e);
                asList = true;
            }
        }
    });
    $effect(() => {
        if (source && sourceDialog && !sourceDialog.open)
            sourceDialog.showModal();
    });
    onDestroy(() => {
        destroyed = true;
        epoch++;
        if (timer) clearInterval(timer);
        renderer?.destroy();
        renderer = null;
    });
</script>

<main class="graph-page">
    {#if !inTauri()}<div class="graph-status">
            Макет окна · заранее записанные ответы на тестовых документах.
            Настоящий worker проверяется отдельно.
        </div>{/if}
    <header>
        <div>
            <small>{app.project?.name} / ЗНАНИЯ И ИСТОЧНИКИ</small>
            <h1>Граф документации</h1>
            <p>Связи, пути и основания в выбранной области.</p>
        </div>
        <button class="ghost" onclick={() => void rebuild()}
            >{status?.active_job
                ? "Остановить задание"
                : "Обновить граф"}</button
        >
    </header>
    <div class="graph-controls">
        <label
            >Задача или тема<input
                bind:value={query}
                onkeydown={(e) => {
                    if (e.key === "Enter") void run();
                }}
                placeholder="Что хотите найти в документации?"
            /></label
        >
        <label
            >Операция<select bind:value={operation}
                >{#each Object.entries(operationLabels) as [key, label]}<option
                        value={key}>{label}</option
                    >{/each}</select
            ></label
        >
        <button class="btn" disabled={busy} onclick={() => void run()}
            >{busy ? "Выполняется…" : "Показать"}</button
        >
    </div>
    {#if operation === "paths"}<div class="graph-controls">
            <label
                >Начало (или выбранный объект)<input
                    bind:value={from}
                    oninput={() => (fromRef = null)}
                /></label
            ><label
                >Конец пути<input
                    bind:value={to}
                    oninput={() => (toRef = null)}
                /></label
            >
        </div>{/if}
    {#if operation === "context"}<div class="graph-controls">
            <label
                >Профиль контекста<select bind:value={taskProfile}
                    ><option value="implementation">Реализация</option><option
                        value="investigation">Исследование</option
                    ><option value="decision">Решение</option></select
                ></label
            >
        </div>{/if}
    {#if operation === "explore"}<div class="graph-controls">
            <label
                >Анализ<select bind:value={analysis}
                    ><option value="topics">Темы</option><option
                        value="connectivity">Изоляты и битые ссылки</option
                    ><option value="coverage">Кандидаты пробелов</option><option
                        value="conflict_candidates">Кандидаты конфликтов</option
                    ></select
                ></label
            ><label
                >Группировка<select bind:value={grouping}
                    ><option value="communities">Сообщества</option><option
                        value="modules">Модули (сравнение)</option
                    ></select
                ></label
            ><label
                ><input type="checkbox" bind:checked={whole} />Вся область</label
            >
        </div>{/if}
    <div class="graph-status" role="status">
        Структура: {reply?.layer_states?.structure
            ? stateLabels[reply.layer_states.structure]
            : stateLabels[status?.state ?? "not_built"]} · Смысловой слой: {stateLabels[
            reply?.layer_state ?? status?.state ?? "not_built"
        ]}. Отсутствие извлечённой связи не доказывает отсутствие зависимости.
        {#if status?.active_job}<span>Задание: {status.active_job}</span>{/if}
        {#if status?.error}<span>{status.error}</span>{/if}
    </div>
    {#if error}<div class="graph-error" role="alert">
            {error}<button class="link" onclick={() => void run()}
                >Повторить</button
            >
        </div>{/if}
    <div class="graph-toolbar">
        <label
            >Вид<select
                aria-label="Вид"
                bind:value={view}
                onchange={(event) => {
                    view = event.currentTarget.value as GraphView;
                    void run();
                }}
                ><option value="documents">Документы</option><option
                    value="sections">Разделы</option
                ><option value="chunks">Чанки</option><option value="entities"
                    >Сущности</option
                ><option value="mixed">Смешанный</option></select
            ></label
        >
        <label
            ><input type="checkbox" bind:checked={local} />Локальный граф</label
        ><label
            >Глубина<input
                type="number"
                min="0"
                max="6"
                bind:value={depth}
            /></label
        >
        <label
            ><input type="checkbox" bind:checked={grouped} />По документам</label
        ><button
            class="ghost"
            aria-pressed={asList}
            onclick={() => (asList = !asList)}>Списком</button
        >
        <span
            >{reply?.nodes.length ?? 0} объектов · {reply?.assertions.length ??
                0} связей в ответе</span
        >
    </div>
    {#if reply?.truncated}<div class="graph-status">
            Ответ усечён: {reply.limit_reasons.join(
                ", ",
            )}.{#if reply.next_cursor}<button
                    class="link"
                    onclick={() => void run(reply!.next_cursor!)}
                    >Следующая страница снимка</button
                >{/if}
        </div>{/if}
    {#if reply?.resolution === "ambiguous"}<section class="graph-candidates">
            <h2>Выберите объект и его источник</h2>
            {#each reply.candidates as n}
                {#if operation === "paths"}
                    <div>
                        {n.label} · {n.namespace}
                        <button
                            class="ghost"
                            onclick={() => {
                                fromRef = n.reference;
                                from = n.label;
                                void run();
                            }}>Начало пути</button
                        >
                        <button
                            class="ghost"
                            onclick={() => {
                                toRef = n.reference;
                                to = n.label;
                                void run();
                            }}>Конец пути</button
                        >
                    </div>
                {:else}<button
                        class="ghost"
                        onclick={() => {
                            selected = n;
                            operation = "relations";
                            void run();
                        }}>{n.label} · {n.namespace}</button
                    >{/if}{/each}
        </section>{/if}
    <div class="graph-workspace">
        <section class="graph-map" aria-label="Карта и список объектов">
            <div class="graph-map-tools">
                <button
                    aria-label="Уменьшить масштаб"
                    title="Уменьшить масштаб"
                    onclick={() => renderer?.zoom(0.8)}><Icon name="minus" size={15} width={1.8} /></button
                ><button
                    aria-label="Увеличить масштаб"
                    title="Увеличить масштаб"
                    onclick={() => renderer?.zoom(1.25)}><Icon name="plus" size={15} width={1.8} /></button
                ><span class="tool-separator"></span><button aria-label="Вписать" title="Вписать граф в панель" onclick={() => renderer?.fit()}
                    ><Icon name="fit" size={15} width={1.8} /></button
                ><button aria-label="Расположить" title="Расположить узлы" onclick={() => renderer?.layout()}
                    ><Icon name="layout" size={15} width={1.8} /></button
                ><button aria-label="Остановить раскладку" title="Остановить раскладку" onclick={() => renderer?.stop()}
                    ><Icon name="stop" size={13} width={1.8} /></button
                >
            </div>
            <div
                class="graph-canvas"
                class:list-mode={asList}
                bind:this={container}
            ></div>
            {#if asList || rendererError}<div class="graph-list">
                    {#if rendererError}<p role="alert">
                            {rendererError} Источники доступны списком.
                        </p>{/if}{#each reply?.nodes ?? [] as n (n.reference.id)}<button
                            class="ghost"
                            class:picked={selected?.reference.id ===
                                n.reference.id}
                            onclick={() => pick(n.reference.id)}
                            >{kindLabels[n.reference.kind]} · {n.label}</button
                        >{/each}{#each reply?.assertions ?? [] as e (e.id)}<button
                            class="ghost"
                            onclick={() => pick(e.id, true)}
                            >{labels(e.from)} → {e.polarity === "negative"
                                ? "не "
                                : ""}{e.predicate} → {labels(e.to)}</button
                        >{/each}
                </div>{/if}
            {#if !busy && reply && !reply.items.length}<p class="graph-empty">
                    В выбранной области нет результатов этой операции.
                </p>{/if}
            <div class="graph-legend">
                ▣ Документ · § Раздел · ● Чанк · ◇ Сущность · ┄ Близость · ⋯
                Отрицание
            </div>
        </section>
        <section class="graph-inspector" aria-label="Источники и анализ">
            {#if edge}<small>УТВЕРЖДЕНИЕ · {edge.origin}</small>
                <h2>
                    {labels(edge.from)} → {edge.polarity === "negative"
                        ? "не "
                        : ""}{edge.predicate} → {labels(edge.to)}
                </h2>
                {#each Object.entries(edge.qualifiers) as [key, value]}<p>
                        {key}: {value}
                    </p>{/each}
            {:else if selected}<small
                    >{kindLabels[selected.reference.kind]} · {selected.namespace}</small
                >
                <h2>{selected.label}</h2>
                <p class="mono">revision: {selected.reference.revision}</p>
                <button
                    class="ghost"
                    onclick={() => {
                        if (selected) {
                            renderer?.pin(selected.reference.id);
                            pinned =
                                renderer?.pinned(selected.reference.id) ??
                                false;
                        }
                    }}
                    >{pinned
                        ? "Освободить позицию"
                        : "Закрепить позицию"}</button
                >
                <div class="graph-actions">
                    {#each ["relations", "paths", "impact"] as op}<button
                            class="ghost"
                            onclick={() => {
                                operation = op as GraphOperation;
                                if (op !== "paths") void run();
                            }}>{operationLabels[op as GraphOperation]}</button
                        >{/each}<button
                        class="link"
                        onclick={() => {
                            selected = null;
                            edge = null;
                        }}>Снять выбор</button
                    >
                </div>
            {:else}<h2>Выберите объект или связь</h2>
                <p>
                    Карта и список используют один снимок. Камера не меняет
                    область поиска.
                </p>{/if}
            {#each proofs as id}{@const ev = reply?.evidence.find(
                    (e) => e.id === id,
                )}{#if ev}<article class="graph-card">
                        <p>
                            {reply?.sources.find(
                                (s) => s.reference.id === ev.document.id,
                            )?.docs_path} · {ev.span.first_line}–{ev.span
                                .last_line}
                        </p>
                        <blockquote>{ev.quote}</blockquote>
                        <button class="link" onclick={() => openProof(ev)}
                            >Открыть основание ↗</button
                        >
                    </article>{/if}{/each}
            {#each reply?.items ?? [] as item}
                {#if item.type === "path" || item.type === "impact"}<article
                        class="graph-card"
                    >
                        <h3>
                            {item.type === "impact"
                                ? item.direct
                                    ? "Прямое влияние"
                                    : "Косвенное влияние"
                                : "Путь"} · {item.path.steps.length} переходов
                        </h3>
                        <p>
                            {item.path.nodes
                                .map((n) => labels(n.id))
                                .join(" → ")}
                        </p>
                        <button
                            class="link"
                            onclick={() =>
                                (highlight = item.path.steps.map(
                                    (s) => s.edge,
                                ))}>Подсветить путь</button
                        >{#each item.path.steps as step}{@const assertion =
                                reply?.assertions.find(
                                    (e) => e.id === step.edge,
                                )}{#if assertion}<p>
                                    {labels(assertion.from)} → {assertion.polarity ===
                                    "negative"
                                        ? "НЕ "
                                        : ""}{assertion.predicate}
                                    → {labels(assertion.to)} · {step.reversed
                                        ? "обратный обход"
                                        : "по направлению"}
                                </p>
                                {#each Object.entries(assertion.qualifiers) as [key, value]}<p
                                    >
                                        {key}: {value}
                                    </p>{/each}
                                {#each assertion.evidence as id}{@const ev =
                                        reply?.evidence.find(
                                            (e) => e.id === id,
                                        )}{#if ev}<button
                                            class="link"
                                            onclick={() => openProof(ev)}
                                            >Основание перехода · {ev.span
                                                .first_line}–{ev.span
                                                .last_line}</button
                                        >{/if}{/each}{/if}{/each}
                    </article>
                {:else if item.type === "topic"}<article class="graph-card">
                        <h3>{item.label}</h3>
                        <p>
                            {item.members.length} документов · {item
                                .representatives.length} представителей
                        </p>
                        <button
                            class="link"
                            onclick={() => {
                                operation = "graph";
                                view = "documents";
                                void run(undefined, item.members);
                            }}>Раскрыть группу</button
                        >
                    </article>
                {:else if item.type === "role_group"}<article
                        class="graph-card"
                    >
                        <h3>{roleLabels[item.role] ?? item.role}</h3>
                        {#each item.evidence as id}{@const ev =
                                reply?.evidence.find(
                                    (e) => e.id === id,
                                )}{#if ev}<blockquote>{ev.quote}</blockquote>
                                <button
                                    class="link"
                                    onclick={() => openProof(ev)}
                                    >Источник роли ↗</button
                                >{/if}{/each}
                    </article>
                {:else if item.type === "audit"}<article class="graph-card">
                        <h3>{item.kind}</h3>
                        <p>{item.detail}</p>
                        {#each item.evidence as id}{@const ev =
                                reply?.evidence.find(
                                    (e) => e.id === id,
                                )}{#if ev}<button
                                    class="link"
                                    onclick={() => openProof(ev)}
                                    >Источник ↗</button
                                >{/if}{/each}
                    </article>
                {:else if item.type === "conflict"}<article class="graph-card">
                        <h3>Кандидат конфликта</h3>
                        <p>
                            Сравните исходники и условия. Это не установленный
                            дефект.
                        </p>
                        {#each item.evidence as id}{@const ev =
                                reply?.evidence.find(
                                    (e) => e.id === id,
                                )}{#if ev}<blockquote>{ev.quote}</blockquote>
                                <button
                                    class="link"
                                    onclick={() => openProof(ev)}
                                    >Сравнить источник ↗</button
                                >{/if}{/each}
                    </article>{/if}
            {/each}
            {#if reply?.missing_roles.length}<p>
                    Не найдены роли: {reply.missing_roles
                        .map((r) => roleLabels[r] ?? r)
                        .join(", ")}. Неполная обработка не доказывает
                    отсутствие источников.
                </p>{/if}
        </section>
    </div>
    <footer>
        {reply
            ? `Снимок ${reply.snapshot.slice(0, 12)} · ${reply.used_bytes} байт · ${reply.elapsed_ms} мс · бюджет ${reply.budget_mode}`
            : "Готовый снимок пока не получен"}{#each reply?.warnings ?? [] as warning}<p
            >
                {warning}
            </p>{/each}
    </footer>
</main>
<dialog
    class="graph-source"
    bind:this={sourceDialog}
    onclose={() => (source = null)}
>
    <button class="ghost" onclick={() => sourceDialog?.close()}>Закрыть</button
    >{#if source}<h2>Основание из снимка</h2>
        <p>
            {sourceMeta?.path} · строки {source.span.first_line}–{source.span
                .last_line}
        </p>
        <pre>{source.quote}</pre>
        <p>Хеш исходника: {source.source_hash}</p>
        <button class="btn" onclick={() => void openOriginal()}
            >Открыть текущий файл</button
        >
        <p>
            Файл мог измениться после снимка; цитата выше относится к указанной
            ревизии.
        </p>{/if}
</dialog>

<style>
    @media (max-width: 1050px) {
        .graph-workspace {
            flex: none !important;
            min-height: 0 !important;
            grid-auto-rows: auto;
        }
        .graph-inspector {
            max-height: none !important;
        }
        .graph-page > footer {
            flex-shrink: 0;
        }
    }
    .graph-page {
        display: flex;
        flex-direction: column;
        min-width: 0;
        min-height: 0;
        overflow: auto;
        padding: 22px 24px;
        gap: 12px;
    }
    .graph-page header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 16px;
    }
    .graph-page h1 {
        font-size: 24px;
        margin: 4px 0;
    }
    .graph-page h2 {
        font-size: 17px;
        overflow-wrap: anywhere;
    }
    .graph-page h3 {
        font-size: 14px;
    }
    .graph-page p {
        margin: 6px 0;
    }
    .graph-page small {
        color: var(--muted);
    }
    .graph-controls,
    .graph-toolbar,
    .graph-actions {
        display: flex;
        gap: 8px;
        align-items: end;
        flex-wrap: wrap;
    }
    .graph-controls label:first-child {
        flex: 1;
    }
    .graph-page label {
        display: flex;
        gap: 5px;
        align-items: center;
        font-size: 12px;
    }
    .graph-controls label {
        flex-direction: column;
        align-items: stretch;
    }
    .graph-page input:not([type="checkbox"]),
    .graph-page select {
        border: 1px solid var(--line-2);
        border-radius: 7px;
        background: var(--panel);
        color: var(--text);
        padding: 8px;
        min-width: 0;
    }
    .graph-toolbar input[type="number"] {
        width: 50px;
    }
    .graph-toolbar > span {
        margin-left: auto;
        font-size: 11px;
        color: var(--muted);
    }
    .graph-status {
        padding: 10px 12px;
        background: var(--panel);
        border: 1px solid var(--line);
        border-left: 3px solid var(--mid);
        border-radius: 8px;
        font-size: 12px;
    }
    .graph-status span {
        display: block;
        overflow-wrap: anywhere;
    }
    .graph-error {
        padding: 12px;
        border: 1px solid var(--bad);
        border-radius: 8px;
        color: var(--bad);
    }
    .graph-workspace {
        display: grid;
        grid-template-columns: minmax(0, 1fr) 330px;
        min-height: 450px;
        flex: 1;
        border: 1px solid var(--line);
        border-radius: 12px;
        overflow: hidden;
    }
    .graph-map {
        position: relative;
        min-width: 0;
        min-height: 420px;
        background: var(--panel-2);
    }
    .graph-canvas {
        position: absolute;
        inset: 45px 0 35px;
    }
    .graph-canvas.list-mode {
        visibility: hidden;
    }
    .graph-map-tools {
        position: relative;
        z-index: 1;
        padding: 2px;
        display: flex;
        align-items: center;
        gap: 1px;
        width: max-content;
        margin: 4px;
        border: 1px solid var(--line);
        border-radius: 5px;
        background: var(--panel);
    }
    .graph-map-tools button {
        display: grid;
        place-items: center;
        width: 20px;
        height: 20px;
        padding: 0;
        border: 0;
        border-radius: 4px;
        background: transparent;
        color: var(--muted);
    }
    .graph-map-tools button:hover { background: var(--accent-soft); color: var(--accent-text); }
    .graph-map-tools button:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
    .tool-separator { width: 1px; height: 12px; background: var(--line); margin: 0 2px; }
    .graph-legend {
        position: absolute;
        bottom: 10px;
        left: 12px;
        font-size: 10px;
        color: var(--muted);
    }
    .graph-inspector {
        padding: 16px;
        overflow: auto;
        max-height: 70vh;
        background: var(--panel);
        border-left: 1px solid var(--line);
        font-size: 12px;
        overflow-wrap: anywhere;
    }
    .graph-card {
        border: 1px solid var(--line);
        border-radius: 8px;
        padding: 10px;
        margin: 10px 0;
    }
    .graph-card blockquote {
        margin: 8px 0;
        padding-left: 9px;
        border-left: 2px solid var(--accent);
        white-space: pre-wrap;
        max-height: 200px;
        overflow: auto;
    }
    .graph-list {
        position: absolute;
        inset: 50px 10px 40px;
        overflow: auto;
        display: flex;
        flex-direction: column;
        gap: 6px;
    }
    .graph-list button {
        text-align: left;
        overflow-wrap: anywhere;
    }
    .graph-list .picked {
        outline: 2px solid var(--accent);
    }
    .graph-page footer {
        font-size: 10px;
        color: var(--muted);
        overflow-wrap: anywhere;
    }
    .graph-empty {
        position: absolute;
        top: 45%;
        left: 15px;
        right: 15px;
        text-align: center;
        color: var(--muted);
        font-size: 13px;
        pointer-events: none;
    }
    .mono {
        font: 10px var(--mono);
        overflow-wrap: anywhere;
    }
    .graph-source {
        background: var(--panel);
        color: var(--text);
        border: 1px solid var(--line-2);
        border-radius: 12px;
        max-width: 700px;
        width: 85vw;
        padding: 22px;
        overflow-wrap: anywhere;
    }
    .graph-source::backdrop {
        background: #0006;
    }
    .graph-source pre {
        white-space: pre-wrap;
        max-height: 50vh;
        overflow: auto;
        background: var(--panel-2);
        padding: 14px;
        font: 12px/1.7 var(--mono);
    }
    @media (max-width: 1050px) {
        .graph-page {
            padding: 14px;
        }
        .graph-workspace {
            grid-template-columns: 1fr;
            overflow: visible;
        }
        .graph-map {
            height: 380px;
            min-height: 380px;
        }
        .graph-inspector {
            max-height: none;
            overflow: visible;
            border-left: 0;
            border-top: 1px solid var(--line);
        }
        .graph-controls label:first-child {
            flex-basis: 100%;
        }
        .graph-toolbar > span {
            margin-left: 0;
        }
    }
    @media (prefers-reduced-motion: reduce) {
        * {
            scroll-behavior: auto;
        }
    }
</style>
