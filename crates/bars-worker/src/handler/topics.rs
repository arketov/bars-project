//! Внутренний API окна для «Новых тем» (`docs/features/new-topics/design.md`, «Внутренние операции»).
//!
//! Методы помечены `Window` в `bars-protocol`: канал закрыт ими для агента и CLI. Здесь — проверка
//! темы текущим поиском, ручная отметка и решения пользователя с ожидаемой ревизией.

use std::path::{Path, PathBuf};

use bars_core::new_topics::{
    Change, CheckRow, NewCheck, NewCheckResult, Observation, Reason, Recorded, Source, StoreError,
    TopicRow, TopicStatus, VariantOutcome, store_path,
};
use bars_protocol::{
    AcceptedOutput, ErrorKind, INTERNAL_ERROR, NewTopicCheck, NewTopicCheckResult, NewTopicDetail,
    NewTopicHit, NewTopicSummary, NewTopicVariant, NewTopicsChangedOutput, NewTopicsCheckParams,
    NewTopicsClearParams, NewTopicsDeleteParams, NewTopicsGenerationOutput, NewTopicsGetParams,
    NewTopicsListOutput, NewTopicsListParams, NewTopicsMarkMissingOutput,
    NewTopicsMarkMissingParams, NewTopicsRenameParams, NewTopicsSetStatusParams,
    NewTopicsSettingsOutput, NewTopicsSettingsParams, RpcError, SearchParams, Target,
};

use super::{WorkerHandler, topic_scope};
use crate::new_topics::{CONTEXT, Origin, Purpose, SearchContext, describe};
use crate::worker::index_token;

/// Самое длинное название темы, которое можно задать вручную.
const MAX_TITLE: usize = 300;

fn topics_error(error: StoreError) -> RpcError {
    match error {
        StoreError::LimitReached(_) | StoreError::InvalidMerge(_) => {
            RpcError::bars(ErrorKind::InvalidParams, describe(&error))
        }
        other => RpcError::standard(INTERNAL_ERROR, describe(&other)),
    }
}

fn stale(message: &str) -> RpcError {
    RpcError::bars(ErrorKind::StaleRef, message)
}

fn changed(change: Change) -> Result<NewTopicsChangedOutput, RpcError> {
    match change {
        Change::Updated { revision } => Ok(NewTopicsChangedOutput { revision }),
        Change::NotFound => Err(stale("тема уже удалена: обновите список")),
        Change::Conflict { .. } => Err(stale("тема изменилась: обновите список и повторите")),
    }
}

/// Область варианта обратно в модули и файлы; обратная к `topic_scope`.
pub(super) fn parse_scope(scope: &str) -> (Vec<String>, Vec<String>) {
    let Some(rest) = scope.strip_prefix("m=") else {
        return (Vec::new(), Vec::new());
    };
    let (modules, files) = rest.split_once(";f=").unwrap_or((rest, ""));
    let split = |text: &str| -> Vec<String> {
        text.split(',')
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
            .collect()
    };
    (split(modules), split(files))
}

fn summary(topic: &TopicRow) -> NewTopicSummary {
    NewTopicSummary {
        id: topic.id,
        revision: topic.revision,
        title: topic.title.clone(),
        status: topic.status.as_str().to_string(),
        first_seen: topic.first_seen,
        last_seen: topic.last_seen,
        resolved_at: topic.resolved_at,
        observations: topic.observations,
        sources: topic.sources.clone(),
        reasons: topic.reasons.clone(),
        variants: topic.variants.len() as u32,
    }
}

fn check_view(check: &CheckRow) -> NewTopicCheck {
    let stamp = check.results.first().map(|r| r.index_stamp.clone());
    let same_index = check
        .results
        .iter()
        .all(|r| Some(&r.index_stamp) == stamp.as_ref());
    let complete = check
        .results
        .iter()
        .all(|r| r.outcome != VariantOutcome::Incomplete);
    NewTopicCheck {
        id: check.id,
        topic_revision: check.topic_revision,
        at: check.at,
        expanded: check.expanded,
        model: check.model.clone(),
        params: check.params.clone(),
        conclusive: !check.results.is_empty() && complete && same_index,
        results: check
            .results
            .iter()
            .map(|r| NewTopicCheckResult {
                variant_id: r.variant_id,
                scope: r.scope.clone(),
                outcome: r.outcome.as_str().to_string(),
                reason: r.reason.clone(),
                index_stamp: r.index_stamp.clone(),
                hits: serde_json::from_str(&r.hits).unwrap_or_default(),
            })
            .collect(),
    }
}

fn detail(topic: &TopicRow) -> NewTopicDetail {
    NewTopicDetail {
        topic: summary(topic),
        variants_list: topic
            .variants
            .iter()
            .map(|v| NewTopicVariant {
                id: v.id,
                text: v.text.clone(),
                scope: v.scope.clone(),
                original_params: v.original_params.clone(),
            })
            .collect(),
        checks: topic.checks.iter().map(check_view).collect(),
    }
}

impl WorkerHandler {
    /// Проект вызова: зарегистрированный корень по абсолютному пути или однозначному имени
    /// (ISS-111: раньше принимался только абсолютный путь).
    fn topics_project(&self, project: &str) -> Result<PathBuf, RpcError> {
        self.worker
            .resolve(project)
            .map_err(super::errors::worker_error)
    }

    fn index_stamp(&self, project: &Path) -> Option<String> {
        self.worker
            .status(project)
            .ok()
            .and_then(|status| status.updated_at.map(index_token))
    }

    pub(super) async fn new_topics_list(
        &self,
        params: NewTopicsListParams,
    ) -> Result<NewTopicsListOutput, RpcError> {
        let project = self.topics_project(&params.project)?;
        let (losses, loss_reason) = self.topics.losses();
        let output = |enabled, generation, topics: &[TopicRow]| NewTopicsListOutput {
            enabled,
            generation,
            topics: topics.iter().map(summary).collect(),
            store_path: store_path(&project).display().to_string(),
            losses,
            loss_reason: loss_reason.clone(),
        };
        let settings = self.topics.settings(&project).await;
        match settings {
            Ok((generation, enabled)) => {
                let topics = self.topics.list(&project).await.map_err(topics_error)?;
                Ok(output(enabled, generation, &topics))
            }
            // Проект ещё без `.bars`: список пуст, и это не ошибка чтения.
            Err(StoreError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(output(true, 0, &[]))
            }
            Err(error) => Err(topics_error(error)),
        }
    }

    pub(super) async fn new_topics_get(
        &self,
        params: NewTopicsGetParams,
    ) -> Result<NewTopicDetail, RpcError> {
        let project = self.topics_project(&params.project)?;
        let topic = self
            .topics
            .get(&project, params.id)
            .await
            .map_err(topics_error)?
            .ok_or_else(|| stale("тема удалена: обновите список"))?;
        Ok(detail(&topic))
    }

    pub(super) async fn new_topics_mark_missing(
        &self,
        params: NewTopicsMarkMissingParams,
    ) -> Result<NewTopicsMarkMissingOutput, RpcError> {
        let project = self.topics_project(&params.project)?;
        let query = params.query.trim();
        if query.is_empty() || params.request_id.is_empty() || params.request_id.len() > 128 {
            return Err(RpcError::bars(
                ErrorKind::InvalidParams,
                "нужны текст запроса и ID отображённого обращения",
            ));
        }
        let event_id = format!("manual:{}", params.request_id);
        let observation = Observation {
            event_id: event_id.clone(),
            generation: 0,
            text: query.to_string(),
            scope: topic_scope(&params.modules, &params.files),
            source: Source::Window,
            reason: Reason::ManualMissing,
            params: "{}".to_string(),
            index_stamp: self.index_stamp(&project),
            vector: None,
            at: crate::mcp_history::now(),
        };
        let recorded = self
            .topics
            .mark_missing(&project, observation)
            .await
            .map_err(topics_error)?;
        match recorded {
            Recorded::Created { topic_id } => Ok(NewTopicsMarkMissingOutput {
                topic_id,
                created: true,
            }),
            Recorded::Merged { topic_id, .. } => Ok(NewTopicsMarkMissingOutput {
                topic_id,
                created: false,
            }),
            Recorded::Duplicate => {
                let topic_id = self
                    .topics
                    .topic_of_event(&project, event_id)
                    .await
                    .map_err(topics_error)?
                    .ok_or_else(|| stale("тема удалена: обновите список"))?;
                Ok(NewTopicsMarkMissingOutput {
                    topic_id,
                    created: false,
                })
            }
            Recorded::Disabled => Err(RpcError::bars(
                ErrorKind::InvalidParams,
                "сбор новых тем выключен: включите его, чтобы добавлять темы",
            )),
            Recorded::StaleGeneration => Err(stale("список очищен во время отметки: повторите")),
        }
    }

    /// Проверка темы: поиск по каждому варианту текущим поиском и текущими настройками. Спроса не
    /// создаёт: назначение «диагностика» отсекает сбор в `search_with_graph`.
    pub(super) async fn new_topics_check(
        &self,
        params: NewTopicsCheckParams,
    ) -> Result<NewTopicDetail, RpcError> {
        let project = self.topics_project(&params.project)?;
        let topic = self
            .topics
            .get(&project, params.id)
            .await
            .map_err(topics_error)?
            .ok_or_else(|| stale("тема удалена: обновите список"))?;
        if topic.revision != params.revision {
            return Err(stale("тема изменилась: обновите список и повторите"));
        }
        let settings = self.settings(&project)?;
        let context = SearchContext {
            origin: Origin::Window,
            purpose: Purpose::Diagnostic,
            request_id: None,
            parent_request_id: None,
        };
        let mut results = Vec::with_capacity(topic.variants.len());
        let mut applied = None;
        let mut variant_params = Vec::with_capacity(topic.variants.len());
        for variant in &topic.variants {
            let (scope, modules, files) = if params.expand {
                ("project".to_string(), Vec::new(), Vec::new())
            } else {
                let (modules, files) = parse_scope(&variant.scope);
                (variant.scope.clone(), modules, files)
            };
            let search = SearchParams {
                diagnostic: true,
                target: Target {
                    cwd: project.display().to_string(),
                    project: Some(project.display().to_string()),
                    wt: None,
                },
                query: variant.text.clone(),
                modules,
                files,
                paths: Vec::new(),
                limit: None,
                candidates: None,
                min_score: None,
                wait_models: None,
            };
            let outcome = CONTEXT
                .scope(context.clone(), self.search_with_graph(search, None, true))
                .await;
            let index_stamp = self.index_stamp(&project);
            let result = match outcome {
                Ok((output, _, _)) => {
                    variant_params.push(serde_json::json!({
                        "variant_id": variant.id,
                        "applied": output.applied,
                        "rerank_error": output.rerank_error,
                    }));
                    let hits: Vec<NewTopicHit> = output
                        .results
                        .iter()
                        // Fallback обычного поиска не является результатом проверки знаний.
                        .filter(|hit| hit.score_kind == bars_protocol::ScoreKind::Rerank)
                        .map(|hit| NewTopicHit {
                            path: hit.path.clone(),
                            title: hit.title.clone(),
                            heading_chain: hit.heading_chain.clone(),
                            first_line: hit.first_line,
                            last_line: hit.last_line,
                            score: hit.score,
                            score_kind: serde_json::to_value(hit.score_kind)
                                .ok()
                                .and_then(|v| v.as_str().map(str::to_owned))
                                .unwrap_or_default(),
                        })
                        .collect();
                    applied.get_or_insert_with(|| {
                        serde_json::to_string(&output.applied).unwrap_or_default()
                    });
                    // Проверка знаний допускает только rerank, даже при непустом fallback.
                    let (kind, reason) = if output.rerank_error.is_some()
                        || output.applied.score_kind != bars_protocol::ScoreKind::Rerank
                    {
                        (VariantOutcome::Incomplete, Some("rerank_unavailable"))
                    } else if output.indexing {
                        (VariantOutcome::Incomplete, Some("indexing"))
                    } else if !hits.is_empty() {
                        (VariantOutcome::Found, None)
                    } else {
                        (VariantOutcome::Empty, None)
                    };
                    NewCheckResult {
                        variant_id: variant.id,
                        scope,
                        outcome: kind,
                        reason: reason.map(str::to_owned),
                        index_stamp,
                        hits: serde_json::to_string(&hits).unwrap_or_else(|_| "[]".into()),
                    }
                }
                Err(error) => NewCheckResult {
                    variant_id: variant.id,
                    scope,
                    outcome: VariantOutcome::Incomplete,
                    reason: Some(
                        match error.kind() {
                            Some(ErrorKind::ModelLoading) => "model_loading",
                            Some(ErrorKind::ReindexRequired) => "reindex_required",
                            Some(ErrorKind::InvalidParams) => "scope_missing",
                            _ => "error",
                        }
                        .to_string(),
                    ),
                    index_stamp,
                    hits: "[]".to_string(),
                },
            };
            results.push(result);
        }
        let mut saved_params: serde_json::Value = applied
            .as_deref()
            .and_then(|value| serde_json::from_str(value).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        saved_params["variants"] = serde_json::Value::Array(variant_params);
        let check = NewCheck {
            topic_revision: topic.revision,
            at: crate::mcp_history::now(),
            expanded: params.expand,
            model: settings.embed.model.clone(),
            params: saved_params.to_string(),
            results,
        };
        // Тему могли удалить или очистить список, пока шла проверка: тогда она не восстанавливается.
        self.topics
            .record_check(&project, topic.id, check)
            .await
            .map_err(topics_error)?
            .ok_or_else(|| stale("тема удалена во время проверки"))?;
        let topic = self
            .topics
            .get(&project, topic.id)
            .await
            .map_err(topics_error)?
            .ok_or_else(|| stale("тема удалена во время проверки"))?;
        Ok(detail(&topic))
    }

    pub(super) async fn new_topics_set_status(
        &self,
        params: NewTopicsSetStatusParams,
    ) -> Result<NewTopicsChangedOutput, RpcError> {
        let project = self.topics_project(&params.project)?;
        let status = match params.status.as_str() {
            "resolved" => TopicStatus::Resolved,
            "open" => TopicStatus::Open,
            _ => {
                return Err(RpcError::bars(
                    ErrorKind::InvalidParams,
                    "решение пользователя — resolved или open",
                ));
            }
        };
        let change = self
            .topics
            .set_status(
                &project,
                params.id,
                params.revision,
                status,
                crate::mcp_history::now(),
            )
            .await
            .map_err(topics_error)?;
        changed(change)
    }

    pub(super) async fn new_topics_rename(
        &self,
        params: NewTopicsRenameParams,
    ) -> Result<NewTopicsChangedOutput, RpcError> {
        let project = self.topics_project(&params.project)?;
        let title = params.title.trim();
        if title.is_empty() || title.chars().count() > MAX_TITLE {
            return Err(RpcError::bars(
                ErrorKind::InvalidParams,
                format!("название темы — от 1 до {MAX_TITLE} знаков"),
            ));
        }
        let change = self
            .topics
            .rename(&project, params.id, params.revision, title.to_string())
            .await
            .map_err(topics_error)?;
        changed(change)
    }

    pub(super) async fn new_topics_merge(
        &self,
        params: bars_protocol::NewTopicsMergeParams,
    ) -> Result<NewTopicsChangedOutput, RpcError> {
        let project = self.topics_project(&params.project)?;
        changed(
            self.topics
                .merge(
                    &project,
                    params.id,
                    params.revision,
                    params.source_id,
                    params.source_revision,
                )
                .await
                .map_err(topics_error)?,
        )
    }

    pub(super) async fn new_topics_delete(
        &self,
        params: NewTopicsDeleteParams,
    ) -> Result<AcceptedOutput, RpcError> {
        let project = self.topics_project(&params.project)?;
        let change = self
            .topics
            .delete_checked(&project, params.id, params.revision)
            .await
            .map_err(topics_error)?;
        changed(change).map(|_| AcceptedOutput { accepted: true })
    }

    pub(super) async fn new_topics_clear(
        &self,
        params: NewTopicsClearParams,
    ) -> Result<NewTopicsGenerationOutput, RpcError> {
        let project = self.topics_project(&params.project)?;
        if !params.confirmed {
            return Err(RpcError::bars(
                ErrorKind::InvalidParams,
                "очистка требует явного подтверждения",
            ));
        }
        let generation = self.topics.clear(&project).await.map_err(topics_error)?;
        Ok(NewTopicsGenerationOutput { generation })
    }

    pub(super) async fn new_topics_settings(
        &self,
        params: NewTopicsSettingsParams,
    ) -> Result<NewTopicsSettingsOutput, RpcError> {
        let project = self.topics_project(&params.project)?;
        let generation = self
            .topics
            .set_enabled(&project, params.enabled)
            .await
            .map_err(topics_error)?;
        Ok(NewTopicsSettingsOutput {
            enabled: params.enabled,
            generation,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_round_trips_through_topic_scope() {
        let modules = vec!["features".to_string(), "adr".to_string()];
        let files = vec!["a/b.md".to_string()];
        assert_eq!(
            parse_scope(&topic_scope(&modules, &files)),
            (vec!["adr".into(), "features".into()], files.clone())
        );
        assert_eq!(
            parse_scope(&topic_scope(&modules, &[])),
            (vec!["adr".into(), "features".into()], Vec::new())
        );
        assert_eq!(parse_scope(&topic_scope(&[], &files)), (Vec::new(), files));
        assert_eq!(
            parse_scope(&topic_scope(&[], &[])),
            (Vec::new(), Vec::new())
        );
    }
}
