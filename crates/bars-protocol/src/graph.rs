use crate::Target;
pub use bars_graph::GraphStatus;
pub use bars_graph::physical::{PhysicalGraphSlice, PhysicalRequest};
pub use bars_graph::{GraphReply, GraphRequest};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphQueryParams {
    #[serde(flatten)]
    pub target: Target,
    pub request: GraphRequest,
}
/// Срез физического графа для окна (`physical-v1`); только окно, MCP и CLI его не получают.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphPhysicalParams {
    #[serde(flatten)]
    pub target: Target,
    #[serde(default)]
    pub request: PhysicalRequest,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphStatusParams {
    #[serde(flatten)]
    pub target: Target,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphBuildParams {
    #[serde(flatten)]
    pub target: Target,
    #[serde(default)]
    pub layer: bars_graph::BuildLayer,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphCancelParams {
    #[serde(flatten)]
    pub target: Target,
    pub job_id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphAcceptedOutput {
    pub accepted: bool,
    pub job_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphSearchParams {
    #[serde(flatten)]
    pub search: crate::SearchParams,
    #[serde(default)]
    pub mode: bars_graph::RetrievalMode,
    #[serde(default)]
    pub graph_budgets: bars_graph::Budgets,
    #[serde(default = "default_related")]
    pub include_related: bool,
    #[serde(default)]
    pub related_budget: Option<u32>,
}
fn default_related() -> bool {
    true
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSearchOutput {
    #[serde(flatten)]
    pub search: crate::SearchOutput,
    pub requested_mode: bars_graph::RetrievalMode,
    pub applied_mode: bars_graph::RetrievalMode,
    pub fallback_reason: Option<String>,
    pub related: Option<GraphReply>,
    pub graph_results: Option<GraphReply>,
}
