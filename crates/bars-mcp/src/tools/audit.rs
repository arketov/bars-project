//! `bars_audit` — аудиты, дубли и новые темы одним инструментом: вид и действие параметрами.
//! Текстовый вид всех действий — строки, время — датой (ISS-113).

use super::*;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AuditArgs {
    /// broken_links, duplicates, contradictions or new_topics. Without kind — the state of all
    /// audit kinds plus the new-topics counter.
    pub kind: Option<String>,
    /// findings (default) — a page of findings, continue with cursor; summary — counts by file,
    /// reason and tag without paging; run — start an audit; cancel — stop a running or queued
    /// audit; get — a saved topic card by id (new_topics); check — re-check a saved topic by id
    /// with the current search (new_topics).
    pub action: Option<String>,
    /// Topic id for get/check (new_topics).
    pub id: Option<i64>,
    /// Expected topic revision for check; without it the current revision is used.
    pub revision: Option<i64>,
    /// Narrow findings or summary by path prefixes (folder of any depth or file).
    #[serde(default)]
    pub r#in: Vec<String>,
    /// Opaque page continuation from the previous findings answer.
    pub cursor: Option<i64>,
    /// Page size for findings (default 50, max 200).
    pub limit: Option<u32>,
    /// Groups hidden by default: ignored, gone, overlap (duplicates).
    #[serde(default)]
    pub show: Vec<String>,
    /// Project: root path or registered name. Default: the session project.
    pub project: Option<String>,
    /// Worktree of the project: a name from `.git/worktrees/<name>` or an absolute path; "." —
    /// the main checkout. Audits and new topics exist only for the main checkout: with a
    /// worktree the answer comes from it, with a note line.
    pub wt: Option<String>,
}

/// Ответ аудита с пометкой worktree строкой над текстом.
fn noted<T: Serialize>(
    note: &Option<String>,
    result: Result<T, ClientError>,
    render: impl FnOnce(&T) -> String,
) -> CallToolResult {
    present_noted(result.map(|output| (output, note.clone())), render)
}

pub(super) fn router() -> ToolRouter<BarsMcp> {
    BarsMcp::audit_tool_router()
}

fn invalid(message: String) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(format!(
        "invalid_params: {message}"
    ))])
}

impl BarsMcp {
    async fn audit_impl(&self, args: AuditArgs, original: Value) -> CallToolResult {
        let kind = args.kind.as_deref();
        let action = args.action.as_deref().unwrap_or("findings");
        if kind.is_none() && args.action.is_none() {
            return self.audit_states(args, original).await;
        }
        let Some(kind) = kind else {
            return invalid("pass kind when action is set".to_string());
        };
        // Аудиты и новые темы есть только у основного checkout: worktree даёт пометку.
        let note = match self
            .audit_worktree_note(args.project.as_deref(), args.wt.as_deref())
            .await
        {
            Ok(note) => note,
            Err(error) => return failure(&error),
        };
        match kind {
            "new_topics" => self.topics_impl(&args, action, original, note).await,
            "broken_links" | "duplicates" | "contradictions" => {
                self.findings_impl(&args, action, original, note).await
            }
            other => invalid(format!(
                "kind is broken_links, duplicates, contradictions or new_topics, not {other}"
            )),
        }
    }

    /// Состояния всех видов аудита плюс счётчик новых тем: без kind и action.
    async fn audit_states(&self, args: AuditArgs, original: Value) -> CallToolResult {
        let note = match self
            .audit_worktree_note(args.project.as_deref(), args.wt.as_deref())
            .await
        {
            Ok(note) => note,
            Err(error) => return failure(&error),
        };
        let project = project_arg(
            &args.project.unwrap_or_else(|| ".".to_string()),
            &self.env.cwd,
        );
        let mut body = match self
            .call_observed(
                &bars_protocol::AuditListParams {
                    project: project.clone(),
                },
                Some(&original),
            )
            .await
        {
            Ok(output) => render::audits_state(&output),
            Err(error) => render::error_part("AUDITS", &error),
        };
        body.push('\n');
        let part = match self
            .call_observed(
                &bars_protocol::NewTopicsListParams { project },
                Some(&original),
            )
            .await
        {
            Ok(output) => render::topics_state(&output),
            Err(error) => render::error_part("NEW TOPICS", &error),
        };
        body.push_str(&part);
        text(with_note(note.as_deref(), body))
    }

    async fn topics_impl(
        &self,
        args: &AuditArgs,
        action: &str,
        original: Value,
        note: Option<String>,
    ) -> CallToolResult {
        let project = project_arg(
            &args.project.clone().unwrap_or_else(|| ".".to_string()),
            &self.env.cwd,
        );
        match action {
            "findings" | "list" => noted(
                &note,
                self.call_observed(
                    &bars_protocol::NewTopicsListParams { project },
                    Some(&original),
                )
                .await,
                render::topics_list,
            ),
            "get" => {
                let Some(id) = args.id else {
                    return invalid("get needs id".to_string());
                };
                noted(
                    &note,
                    self.call_observed(
                        &bars_protocol::NewTopicsGetParams { project, id },
                        Some(&original),
                    )
                    .await,
                    render::topic_detail,
                )
            }
            "check" => {
                let Some(id) = args.id else {
                    return invalid("check needs id".to_string());
                };
                let revision = match args.revision {
                    Some(revision) => revision,
                    None => match self
                        .call_observed(
                            &bars_protocol::NewTopicsGetParams {
                                project: project.clone(),
                                id,
                            },
                            Some(&original),
                        )
                        .await
                    {
                        Ok(detail) => detail.topic.revision,
                        Err(error) => return failure(&error),
                    },
                };
                noted(
                    &note,
                    self.call_observed(
                        &bars_protocol::NewTopicsCheckParams {
                            project,
                            id,
                            revision,
                            expand: false,
                        },
                        Some(&original),
                    )
                    .await,
                    render::topic_detail,
                )
            }
            other => invalid(format!(
                "new_topics supports actions findings, get, check — not {other}"
            )),
        }
    }

    async fn findings_impl(
        &self,
        args: &AuditArgs,
        action: &str,
        original: Value,
        note: Option<String>,
    ) -> CallToolResult {
        let kind = args.kind.clone().expect("kind checked above");
        let project = project_arg(
            &args.project.clone().unwrap_or_else(|| ".".to_string()),
            &self.env.cwd,
        );
        match action {
            "findings" => {
                let show = &args.show;
                let unknown = show
                    .iter()
                    .find(|group| !matches!(group.as_str(), "ignored" | "gone" | "overlap"));
                if let Some(group) = unknown {
                    return invalid(format!(
                        "show groups are ignored, gone, overlap — not {group}"
                    ));
                }
                let paths = match scope_paths(&args.r#in) {
                    Ok(paths) => paths,
                    Err(message) => {
                        return CallToolResult::error(vec![ContentBlock::text(message)]);
                    }
                };
                noted(
                    &note,
                    self.call_observed(
                        &bars_protocol::AuditFindingsParams {
                            project,
                            kind,
                            group: show
                                .iter()
                                .any(|group| group == "overlap")
                                .then(|| "overlap".to_string()),
                            include_gone: show.iter().any(|group| group == "gone"),
                            include_ignored: show.iter().any(|group| group == "ignored"),
                            cursor: args.cursor,
                            limit: args.limit,
                            paths,
                        },
                        Some(&original),
                    )
                    .await,
                    render::findings,
                )
            }
            "summary" => {
                let paths = match scope_paths(&args.r#in) {
                    Ok(paths) => paths,
                    Err(message) => {
                        return CallToolResult::error(vec![ContentBlock::text(message)]);
                    }
                };
                noted(
                    &note,
                    self.call_observed(
                        &bars_protocol::AuditSummaryParams {
                            project,
                            kind,
                            paths,
                        },
                        Some(&original),
                    )
                    .await,
                    render::audit_summary,
                )
            }
            "run" => noted(
                &note,
                self.call_observed(
                    &bars_protocol::AuditRunParams { project, kind },
                    Some(&original),
                )
                .await,
                render::audit_run,
            ),
            "cancel" => noted(
                &note,
                self.call_observed(
                    &bars_protocol::AuditCancelParams { project, kind },
                    Some(&original),
                )
                .await,
                render::audit_cancel,
            ),
            other => invalid(format!(
                "audit kinds support actions findings, summary, run, cancel — not {other}"
            )),
        }
    }
}

#[tool_router(router = audit_tool_router)]
impl BarsMcp {
    #[tool(
        name = "bars_audit",
        annotations(read_only_hint = false),
        description = "Knowledge audits, duplicates and saved New Topics in one tool. \
                       kind: broken_links, duplicates, contradictions, new_topics; without kind \
                       — the state of all kinds plus the topics counter. Actions: findings \
                       (default; page with cursor, limit; show=ignored, gone, overlap adds hidden \
                       groups; in narrows by path prefixes and returns counts by file), summary \
                       (counts by file, reason and tag), run, cancel (not for new_topics); \
                       new_topics: list, get and check by id (revision is taken current unless \
                       given). Audits read the shared store, never edit Markdown. Audits and new \
                       topics exist only for the main checkout: with wt (a git worktree) the \
                       answer comes from the main checkout, with a note line. Dates are printed \
                       as dates."
    )]
    async fn bars_audit(
        &self,
        original: observation::Original,
        Parameters(args): Parameters<AuditArgs>,
    ) -> CallToolResult {
        self.audit_impl(args, original.0).await
    }
}
