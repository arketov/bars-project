//! `bars_status` — одна сводка проекта: проект и модули, индекс, версии прокси и воркера,
//! аудиты, новые темы, граф. Ошибка одной части печатается в этой части и не отменяет остальные
//! (ISS-110); расхождение сборок прокси и воркера видно (ISS-117).

use super::*;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct StatusArgs {
    /// Project: root path or registered name. Default: the session project.
    pub project: Option<String>,
    /// true — the whole registry of projects instead of one project.
    #[serde(default)]
    pub all: bool,
    /// true — start an index freshness check first (what the window and bars-cli reindex do
    /// without full). Progress shows in the INDEX part.
    #[serde(default)]
    pub refresh: bool,
}

pub(super) fn router() -> ToolRouter<BarsMcp> {
    BarsMcp::status_tool_router()
}

impl BarsMcp {
    async fn status_impl(&self, args: StatusArgs, original: Value) -> CallToolResult {
        if args.all && args.project.is_some() {
            return CallToolResult::error(vec![ContentBlock::text(
                "invalid_params: all=true cannot be combined with project",
            )]);
        }
        let target = self.target(args.project.clone());
        let mut body = String::new();
        if args.refresh {
            let params = bars_protocol::ReindexParams {
                target: target.clone(),
                full: false,
                accept_model_change: false,
            };
            let part = match self.call_observed(&params, Some(&original)).await {
                Ok(output) => render::refresh(&output),
                Err(error) => render::error_part("REFRESH", &error),
            };
            body.push_str(&part);
            body.push('\n');
        }
        // Проект: путь нужен частям аудитов и тем, у которых нет Target с cwd. Неразрешимый
        // проект — ошибка вызова: остальные части всё равно были бы про тот же проект.
        let projects_params = bars_protocol::ProjectsParams {
            target: (!args.all).then(|| target.clone()),
        };
        let mut lazy = false;
        let project_path = match self.call_observed(&projects_params, Some(&original)).await {
            Ok(output) => {
                let path = output.projects.first().map(|project| project.path.clone());
                // Подпроект, которого ещё не требовали: индекса нет, остальные части не зовутся.
                lazy = !args.all
                    && output
                        .projects
                        .first()
                        .is_some_and(|project| project.state == bars_protocol::ProjectState::Lazy);
                body.push_str(&render::projects(&output));
                path
            }
            Err(error) => return failure(&error),
        };
        let index = bars_protocol::StatusParams {
            target: target.clone(),
        };
        let part = match self.call_observed(&index, Some(&original)).await {
            Ok(output) => render::status(&output),
            Err(error) => render::error_part("INDEX", &error),
        };
        body.push('\n');
        body.push_str(&part);
        body.push('\n');
        body.push_str(&render::versions(
            env!("CARGO_PKG_VERSION"),
            bars_protocol::BUILD_ID,
            self.worker_build().await,
        ));
        if lazy {
            // Аудиты, новые темы и граф лениво не вызываются: пассивный вызов не должен будить проект.
            body.push_str(
                "\nnot indexed yet: any search, read or refresh=true in this project starts the first index",
            );
            return text(body);
        }
        // Аудиты и новые темы: явный project (имя или путь) или путь из части PROJECT.
        // При all=true часть PROJECT — реестр проектов: аудиты и темы за одним проектом
        // под ней были бы произвольным выбором, поэтому не выводятся.
        let audit_project = if args.all {
            None
        } else {
            args.project
                .clone()
                .or_else(|| project_path.clone())
                .map(|value| project_arg(&value, &self.env.cwd))
        };
        if let Some(project) = audit_project {
            let params = bars_protocol::AuditListParams {
                project: project.clone(),
            };
            let part = match self.call_observed(&params, Some(&original)).await {
                Ok(output) => render::audits_state(&output),
                Err(error) => render::error_part("AUDITS", &error),
            };
            body.push('\n');
            body.push_str(&part);
            let params = bars_protocol::NewTopicsListParams { project };
            let part = match self.call_observed(&params, Some(&original)).await {
                Ok(output) => render::topics_state(&output),
                Err(error) => render::error_part("NEW TOPICS", &error),
            };
            body.push('\n');
            body.push_str(&part);
        } else {
            body.push_str(
                "\n=== AUDITS ===\naudits and new topics need one project: repeat with project=, or drop all=true\n=== END AUDITS ===",
            );
        }
        let graph = bars_protocol::GraphStatusParams { target };
        let part = match self.call_observed(&graph, Some(&original)).await {
            Ok(output) => format!(
                "=== GRAPH ==={}\n=== END GRAPH ===",
                render::graph_part(&output)
            ),
            Err(error) => render::error_part("GRAPH", &error),
        };
        body.push('\n');
        body.push_str(&part);
        text(body)
    }
}

#[tool_router(router = status_tool_router)]
impl BarsMcp {
    #[tool(
        name = "bars_status",
        annotations(read_only_hint = true),
        description = "One project summary in independent parts: PROJECT (path, documentation \
                       root, modules with document counts and purpose), INDEX (state, updated, \
                       queue, models, errors), VERSIONS (proxy and worker builds with a warning \
                       when they differ), AUDITS (kind, state, date, finding counts), NEW TOPICS, \
                       GRAPH. An error in one part is printed in that part and does not cancel \
                       the rest. refresh=true starts an index freshness check first. \
                       all=true lists all registered projects. When the index needs reindexing \
                       only the user can start it: tell the user, do not retry."
    )]
    async fn bars_status(
        &self,
        original: observation::Original,
        Parameters(args): Parameters<StatusArgs>,
    ) -> CallToolResult {
        self.status_impl(args, original.0).await
    }
}
