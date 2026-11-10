use super::*;
use crate::mcp_history::{Attempt, History};
use serde_json::json;
use std::cell::RefCell;

tokio::task_local! { static APPLIED: RefCell<Value>; }
pub(super) fn applied_field(key: &str, value: Value) {
    let _ = APPLIED.try_with(|a| {
        a.borrow_mut()[key] = value;
    });
}
impl WorkerHandler {
    pub(super) async fn observed(
        &self,
        session: &str,
        client: ClientKind,
        call: Call,
        original: Option<Value>,
    ) -> Result<Value, RpcError> {
        if client != ClientKind::Mcp {
            return self.handle_plain(client, call).await;
        }
        let (tool, target, input, explicit_project) = match &call {
            Call::GraphSearch(p) => ("bars_search", Some(&p.search.target), output_value(p), None),
            Call::GraphQuery(p) => (
                match p.request.operation {
                    bars_core::graph::Operation::Relations => "bars_relations",
                    bars_core::graph::Operation::Paths => "bars_paths",
                    bars_core::graph::Operation::Impact => "bars_impact",
                    bars_core::graph::Operation::Context => "bars_context",
                    bars_core::graph::Operation::Explore => "bars_explore",
                    bars_core::graph::Operation::Graph => "bars_graph",
                },
                Some(&p.target),
                output_value(p),
                None,
            ),
            Call::Search(p) => ("bars_search", Some(&p.target), output_value(p), None),
            Call::Similar(p) => ("bars_search", Some(&p.target), output_value(p), None),
            Call::Like(p) => ("bars_search", Some(&p.target), output_value(p), None),
            Call::Read(p) => ("bars_read", Some(&p.target), output_value(p), None),
            Call::DiskText(p) => ("bars_search", Some(&p.target), output_value(p), None),
            Call::DiskLinks(p) => ("bars_read", Some(&p.target), output_value(p), None),
            Call::Outline(p) => ("bars_read", Some(&p.target), output_value(p), None),
            Call::Status(p) => ("bars_status", Some(&p.target), output_value(p), None),
            Call::Reindex(p) => ("bars_status", Some(&p.target), output_value(p), None),
            Call::Projects(p) => ("bars_status", p.target.as_ref(), output_value(p), None),
            Call::AuditList(p) => ("bars_audit", None, output_value(p), Some(p.project.clone())),
            Call::AuditSummary(p) => {
                ("bars_audit", None, output_value(p), Some(p.project.clone()))
            }
            Call::AuditRun(p) => ("bars_audit", None, output_value(p), Some(p.project.clone())),
            Call::AuditCancel(p) => {
                ("bars_audit", None, output_value(p), Some(p.project.clone()))
            }
            Call::AuditFindings(p) => {
                ("bars_audit", None, output_value(p), Some(p.project.clone()))
            }
            Call::NewTopicsList(p) => {
                ("bars_audit", None, output_value(p), Some(p.project.clone()))
            }
            Call::NewTopicsGet(p) => {
                ("bars_audit", None, output_value(p), Some(p.project.clone()))
            }
            Call::NewTopicsCheck(p) => {
                ("bars_audit", None, output_value(p), Some(p.project.clone()))
            }
            _ => return self.handle_plain(client, call).await,
        };
        let preparation = std::time::Instant::now();
        let project = target
            .and_then(|t| self.resolve(t).ok())
            .or_else(|| explicit_project.and_then(|p| self.worker.resolve(&p).ok()));
        if let Some(project) = &project {
            self.worker
                .observe_mcp(project)
                .map_err(super::worker_error)?;
        }
        let path = project
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let name = project
            .as_ref()
            .and_then(|p| self.worker.project_metadata(p).map(|(name, _)| name));
        let complete = original.is_some();
        let requested = crate::mcp_history::requested(tool, original.as_ref().unwrap_or(&input));
        let history = self.history.clone();
        let h = history.clone();
        let session = session.to_owned();
        let prepared = preparation.elapsed();
        let token = if let Some(permit) = history.write_permit() {
            match tokio::task::spawn_blocking(move || {
                let _permit = permit;
                h.observe(h.begin(&session, tool, &path, name, requested, complete))
            })
            .await
            {
                Ok(value) => value.flatten(),
                Err(_) => {
                    history.observe::<()>(Err(rusqlite::Error::InvalidQuery));
                    None
                }
            }
        } else {
            None
        };
        // Это только применённые поля typed-вызова; недоступные значения не восстанавливаются.
        let mut scope = json!({"format": original.as_ref().map(|v|v.get("format").and_then(Value::as_str).unwrap_or("txt"))});
        if let Some(t) = target {
            scope["cwd"] = json!(t.cwd);
        }
        // Номер попытки прокси при `model_loading`: повтор прокси, а не агента (ISS-31).
        if let Some(attempt) = original
            .as_ref()
            .and_then(|v| v.get("proxy_attempt"))
            .and_then(Value::as_u64)
        {
            scope["proxy_attempt"] = json!(attempt);
        }
        match &call {
            Call::DiskText(p) => {
                scope["modules"] = json!(p.modules);
                scope["files"] = json!(p.files);
                scope["paths"] = json!(p.paths);
                scope["path"] = json!(p.path);
                scope["offset"] = json!(p.offset);
                scope["revision"] = json!(p.revision);
                scope["case_sensitive"] = json!(p.case_sensitive);
                scope["regex"] = json!(p.regex);
                scope["path_filter"] = json!(p.path_filter);
            }
            Call::DiskLinks(p) => {
                scope["path"] = json!(p.path);
                scope["direction"] = json!(p.direction);
                scope["cursor_present"] = json!(p.cursor.is_some());
            }
            Call::GraphSearch(p) => {
                scope["diagnostic"] = json!(p.search.diagnostic);
                scope["paths"] = json!(p.search.paths);
                scope["mode"] = json!(p.mode);
                scope["include_related"] = json!(p.include_related);
                scope["related_budget"] = json!(p.related_budget);
            }
            Call::GraphQuery(p) => {
                scope["operation"] = json!(p.request.operation);
                scope["modules"] = json!(p.request.scope.modules);
                scope["files"] = json!(p.request.scope.files);
            }
            Call::Search(p) => {
                scope["diagnostic"] = json!(p.diagnostic);
                scope["paths"] = json!(p.paths);
            }
            Call::Similar(p) => {
                scope["paths"] = json!(p.paths);
                scope["exclude_document"] = json!(p.exclude_document);
            }
            Call::Like(p) => {
                scope["paths"] = json!(p.paths);
            }
            Call::Outline(p) => {
                scope["levels"] = if p.levels.is_empty() {
                    json!([1, 2, 3, 4, 5, 6])
                } else {
                    json!(p.levels)
                };
                scope["under"] = json!(p.under);
                scope["under_line"] = json!(p.under_line);
            }
            Call::Reindex(p) => scope["full"] = json!(p.full),
            Call::AuditFindings(p) => {
                scope["kind"] = json!(p.kind);
                scope["group"] = json!(p.group);
                scope["paths"] = json!(p.paths);
            }
            Call::AuditSummary(p) => {
                scope["kind"] = json!(p.kind);
                scope["paths"] = json!(p.paths);
            }
            Call::AuditRun(p) => scope["kind"] = json!(p.kind),
            Call::AuditCancel(p) => scope["kind"] = json!(p.kind),
            _ => {}
        }
        let start = std::time::Instant::now();
        let (result, applied) = APPLIED
            .scope(RefCell::new(scope), async {
                let result = self.handle_plain(client, call).await;
                let applied = APPLIED.with(|v| v.borrow().clone());
                (result, applied)
            })
            .await;
        let elapsed = (start.elapsed() + prepared)
            .as_millis()
            .min(u64::MAX as u128) as u64;
        if let Some(mut a) = token {
            a.finished = Some(crate::mcp_history::now());
            a.duration_ms = Some(elapsed);
            complete_attempt(&mut a, tool, &input, applied, &result);
            let h = history.clone();
            let Some(permit) = history.write_permit() else {
                return result;
            };
            if tokio::task::spawn_blocking(move || {
                let _permit = permit;
                h.observe(h.finish(&a));
            })
            .await
            .is_err()
            {
                history.observe::<()>(Err(rusqlite::Error::InvalidQuery));
            }
        }
        result
    }
    pub(super) async fn handle_plain(
        &self,
        _client: ClientKind,
        call: Call,
    ) -> Result<Value, RpcError> {
        if call.window_only() {
            return Err(RpcError::standard(
                METHOD_NOT_FOUND,
                format!("нет метода {}", call.method()),
            ));
        }
        // Вызов пришёл по каналу: окно ходит в `dispatch` напрямую, значит это агент или CLI.
        let origin = match _client {
            ClientKind::Mcp => crate::new_topics::Origin::Mcp,
            ClientKind::Cli => crate::new_topics::Origin::Cli,
        };
        let context = crate::new_topics::SearchContext {
            origin,
            request_id: Some(self.topics.next_request_id(origin)),
            ..Default::default()
        };
        crate::new_topics::CONTEXT
            .scope(context, self.dispatch(call))
            .await
    }
    pub fn mcp_history(&self) -> History {
        self.history.clone()
    }
}

fn complete_attempt(
    a: &mut Attempt,
    tool: &str,
    input: &Value,
    mut applied: Value,
    result: &Result<Value, RpcError>,
) {
    a.project = applied["resolved_project"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    a.project_name = applied["resolved_project_name"].as_str().map(str::to_owned);
    match result {
        Err(e) => {
            if tool == "bars_read" {
                applied["cursor_validation"] = json!(match e.kind() {
                    Some(ErrorKind::StaleCursor) => "stale",
                    Some(ErrorKind::InvalidParams) => "invalid_params",
                    _ => "not_completed",
                });
            }
            a.outcome = "error".into();
            a.error = json!({"code":e.code,"kind":e.kind().map(|k|k.name()),"stage":"worker","message":safe_error(e.kind())});
            a.result = json!({"count":null,"unit":"unknown"});
        }
        Ok(out) => {
            a.outcome = "success".into();
            a.result = match tool {
                "bars_search" => {
                    if let Some(hits) = out["hits_total"].as_i64() {
                        // Дисковый точный поиск (occurrences): ответ DiskTextOutput плоский.
                        applied["search"] = out["applied"].clone();
                        applied["clamped"] = out["clamped"].clone();
                        json!({"count":hits,"unit":"occurrences","skipped_total":out["skipped_total"],"limit_reasons":out["limit_reasons"]})
                    } else if out.get("source").is_some() {
                        // like/similar: похожие чанки без баллов по сигналам.
                        for k in ["limit", "clamped", "indexing", "pending"] {
                            applied[k] = out[k].clone();
                        }
                        applied["source_chunk"] = out["source"]["chunk_id"].clone();
                        applied["score_kind"] = json!("cosine");
                        if let Some(linked) = out.get("linked_from") {
                            applied["linked_from"] = linked.clone();
                        }
                        applied["sources"] = json!({"limit": match input.get("limit").filter(|v| !v.is_null()) {
                            None => "default",
                            Some(v) if *v == out["limit"] => "explicit",
                            _ => "clamped",
                        }});
                        json!({"count":out["results"].as_array().map(Vec::len),"unit":"fragments","warning":false})
                    } else {
                        applied["scope"] = out["applied"]["scope"].clone();
                        applied["by_module"] = out["by_module"].clone();
                        if out.get("applied_mode").is_some() {
                            applied["mode"] = out["applied_mode"].clone();
                        }
                        applied["search"] = out["applied"].clone();
                        applied["indexing"] = out["indexing"].clone();
                        applied["pending"] = out["pending"].clone();
                        applied["rerank_performed"] = json!(out["applied"]["score_kind"] == "rerank");
                        applied["rerank_fallback"] = json!(out.get("rerank_error").is_some());
                        applied["semantic_fallback"] = json!(out.get("semantic_error").is_some());
                        let mut sources = serde_json::Map::new();
                        for (request, effective) in [
                            ("limit", "results"),
                            ("candidates", "candidates"),
                            ("min_score", "min_score"),
                        ] {
                            let explicit = input.get(request).filter(|v| !v.is_null());
                            sources.insert(
                                request.into(),
                                json!(match explicit {
                                    None => "default",
                                    Some(v) if *v == out["applied"][effective] => "explicit",
                                    _ => "clamped",
                                }),
                            );
                        }
                        applied["sources"] = Value::Object(sources);
                        if out.get("applied_mode").is_some_and(|v| v == "graph") {
                            json!({"count":out["graph_results"]["items"].as_array().map(Vec::len),"unit":"graph_items"})
                        } else {
                            json!({"count":out["results"].as_array().map(Vec::len),"unit":"fragments","warning":out.get("rerank_error").is_some() || out.get("semantic_error").is_some()})
                        }
                    }
                }
                "bars_read" => {
                    if out.get("items").is_some() {
                        // view=links.
                        applied["limit"] = out["applied_limit"].clone();
                        applied["clamped"] = out["clamped"].clone();
                        applied["cursor_validation"] = json!("valid_or_absent");
                        json!({"count":out["items"].as_array().map(Vec::len),"unit":"links","has_more":out["has_more"],"limit_reasons":out["limit_reasons"]})
                    } else if out.get("headings").is_some() {
                        json!({"count":out["headings"].as_array().map(Vec::len),"unit":"headings"})
                    } else {
                        for key in ["lines_total", "range", "sections"] {
                            if let Some(value) = out.get(key) {
                                applied[key] = value.clone();
                            }
                        }
                        let count = if let Some(range) = out.get("range") {
                            range["to"]
                                .as_u64()
                                .zip(range["from"].as_u64())
                                .map(|(to, from)| to - from + 1)
                        } else if let Some(sections) = out["sections"].as_array() {
                            Some(
                                sections
                                    .iter()
                                    .map(|s| {
                                        s["to"].as_u64().unwrap_or(0) - s["from"].as_u64().unwrap_or(0)
                                            + 1
                                    })
                                    .sum(),
                            )
                        } else {
                            out["text"]
                                .as_str()
                                .map(|s| bars_protocol::count_lines(s) as u64)
                        };
                        json!({"count":count,"unit":"lines"})
                    }
                }
                "bars_status" => {
                    if let Some(projects) = out.get("projects") {
                        applied["all"] = json!(input.get("target").is_none());
                        json!({"count":projects.as_array().map(Vec::len),"unit":"projects"})
                    } else if out.get("accepted").is_some() && input.get("full").is_some() {
                        // refresh=true: проверка актуальности.
                        for k in ["accepted", "job_id", "operation_id", "merged"] {
                            if let Some(v) = out.get(k) {
                                applied[k] = v.clone();
                            }
                        }
                        json!({"count":null,"unit":"not_applicable","queued":true})
                    } else {
                        for k in ["state", "updated_at", "pending", "watching", "model"] {
                            applied[k] = out[k].clone();
                        }
                        applied["has_problem"] =
                            json!(out.get("last_error").is_some_and(|v| !v.is_null()));
                        json!({"count":null,"unit":"not_applicable"})
                    }
                }
                "bars_audit" => {
                    if out.get("audits").is_some() {
                        applied["reindex_required"] = out["reindex_required"].clone();
                        applied["indexing"] = out["indexing"].clone();
                        json!({"count":out["audits"].as_array().map(Vec::len),"unit":"audit_kinds"})
                    } else if out.get("items").is_some() {
                        for k in ["run_id", "total", "next"] {
                            applied[k] = out[k].clone();
                        }
                        json!({"count":out["items"].as_array().map(Vec::len),"unit":"findings","total":out["total"]})
                    } else if out.get("files").is_some() {
                        for k in ["run_id", "total"] {
                            applied[k] = out[k].clone();
                        }
                        json!({"count":out["total"],"unit":"findings"})
                    } else if out.get("topics").is_some() {
                        applied["enabled"] = out["enabled"].clone();
                        json!({"count":out["topics"].as_array().map(Vec::len),"unit":"topics"})
                    } else if out.get("accepted").is_some() || out.get("cancelled").is_some() {
                        for k in ["accepted", "already", "queued", "cancelled"] {
                            if let Some(v) = out.get(k) {
                                applied[k] = v.clone();
                            }
                        }
                        json!({"count":null,"unit":"not_applicable"})
                    } else {
                        // Карточка темы (get/check).
                        applied["status"] = out["status"].clone();
                        json!({"count":out["observations"],"unit":"observations"})
                    }
                }
                _ => {
                    a.incomplete
                        .push("graph_detail_contract_not_recorded".into());
                    json!({"count":out["items"].as_array().map(Vec::len),"unit":"graph_items"})
                }
            };
        }
    }
    let mut budget = 8000;
    a.applied = crate::mcp_history::bounded(applied, "applied", &mut budget, &mut a.incomplete);
}

fn safe_error(kind: Option<ErrorKind>) -> &'static str {
    match kind {
        Some(ErrorKind::ProjectNotFound) => "Проект не найден",
        Some(ErrorKind::ProjectUnavailable) => "Папка проекта недоступна",
        Some(ErrorKind::FileNotFound) => "Документ не найден",
        Some(ErrorKind::InvalidParams) => "Недопустимые параметры вызова",
        Some(ErrorKind::ProviderError) => "Ошибка модели или провайдера",
        Some(ErrorKind::ModelLoading) => "Модель ещё не готова",
        Some(ErrorKind::ChunkNotFound) => "Чанк не найден или устарел",
        Some(ErrorKind::ReindexRequired) => "Требуется переиндексация",
        Some(ErrorKind::ReindexNeedsConsent) => "Требуется согласие на переиндексацию",
        _ => "Вызов не выполнен; подробности определяются кодом ошибки",
    }
}

pub(super) fn outline_anchor(text: &str, query: &OutlineQuery) -> Option<Value> {
    let under = query.under.as_ref()?;
    let all = outline(text, &OutlineQuery::default()).ok()?;
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut anchors = Vec::new();
    for h in all {
        while stack.last().is_some_and(|(level, _)| *level >= h.level) {
            stack.pop();
        }
        stack.push((h.level, h.text.clone()));
        let chain = stack
            .iter()
            .map(|(_, text)| text.as_str())
            .collect::<Vec<_>>()
            .join("/");
        let matched = match under {
            Under::Line(n) => *n == h.line,
            Under::Chain(c) => c.trim() == chain,
        };
        if matched {
            anchors.push(json!({"line":h.line,"heading_chain":chain}));
        }
    }
    Some(json!(anchors))
}
