use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const METHOD_VERSION: &str = "explicit-markdown-v6";
pub const DTO_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildLayer {
    #[default]
    All,
    Structure,
    Semantic,
    Similarity,
}

fn legacy_similarity_state() -> LayerState {
    LayerState::Partial
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Kind {
    Document,
    Section,
    Chunk,
    Entity,
    Community,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ObjectRef {
    pub kind: Kind,
    pub id: String,
    pub revision: String,
}

pub fn object_ref(namespace: &str, kind: Kind, key: &str, revision: &str) -> ObjectRef {
    let payload = serde_json::to_vec(&(namespace, kind, key)).expect("ref сериализуется");
    ObjectRef {
        kind,
        id: blake3::hash(&payload).to_hex().to_string(),
        revision: revision.into(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub first_line: usize,
    pub last_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceMeta {
    pub reference: ObjectRef,
    pub path: String,
    pub docs_path: String,
    pub module: String,
    pub title: String,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceDocument {
    pub meta: SourceMeta,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub id: String,
    pub document: ObjectRef,
    pub span: Span,
    pub quote: String,
    pub source_hash: String,
    pub method: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub reference: ObjectRef,
    pub label: String,
    pub namespace: String,
    pub aliases: Vec<String>,
    pub source_ids: Vec<String>,
    pub evidence: Vec<String>,
    pub roles: Vec<Role>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Predicate {
    Contains,
    Covers,
    NextChunk,
    LinksTo,
    Mentions,
    DependsOn,
    Uses,
    Implements,
    Verifies,
    Supersedes,
    /// Legacy input filter; extraction emits DependsOn with kind=precondition.
    Requires,
    SimilarTo,
    CoOccurs,
    MemberOf,
}

impl Predicate {
    pub fn semantic(self) -> bool {
        matches!(
            self,
            Self::DependsOn
                | Self::Uses
                | Self::Implements
                | Self::Verifies
                | Self::Supersedes
                | Self::Requires
        )
    }
    pub fn impact(self) -> bool {
        matches!(
            self,
            Self::DependsOn | Self::Uses | Self::Implements | Self::Verifies | Self::Requires
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Polarity {
    Positive,
    Negative,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Structural,
    ExplicitRule,
    Similarity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub id: String,
    pub from: String,
    pub to: String,
    pub predicate: Predicate,
    pub polarity: Polarity,
    pub qualifiers: BTreeMap<String, String>,
    pub origin: Origin,
    pub evidence: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Role {
    Requirement,
    Decision,
    Behavior,
    Architecture,
    Constraint,
    Verification,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum TaskProfile {
    Implementation,
    Investigation,
    Decision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerState {
    Disabled,
    NotBuilt,
    Building,
    Ready,
    Partial,
    Stale,
    Failed,
    Cancelled,
}
impl LayerState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disabled => "выключен",
            Self::NotBuilt => "не построен",
            Self::Building => "строится",
            Self::Ready => "готов",
            Self::Partial => "частичный",
            Self::Stale => "устарел",
            Self::Failed => "ошибка",
            Self::Cancelled => "отменён",
        }
    }

    /// Причина, по которой граф в этом состоянии не читается: хвост сообщения «граф ещё не готов: …».
    pub fn not_ready_reason(self) -> &'static str {
        match self {
            Self::Disabled => "слой графа выключен",
            Self::NotBuilt => "граф не построен",
            Self::Building => "граф строится, готового снимка ещё нет",
            Self::Stale => "документы изменились после построения, граф ждёт обновления",
            Self::Failed => "построение графа завершилось ошибкой, готового снимка нет",
            Self::Cancelled => "построение графа остановлено, готового снимка нет",
            Self::Ready | Self::Partial => "готового снимка графа нет",
        }
    }

    /// Предупреждение о снимке, возвращённом вместо результата фонового задания.
    pub fn background_job_warning(self) -> &'static str {
        match self {
            Self::Building => {
                "Граф сейчас перестраивается; ответ построен по предыдущему согласованному снимку тех же исходников"
            }
            Self::Cancelled => {
                "Последнее построение графа остановлено; ответ построен по предыдущему согласованному снимку тех же исходников"
            }
            Self::Failed => {
                "Последнее построение графа завершилось ошибкой; ответ построен по предыдущему согласованному снимку тех же исходников"
            }
            _ => "Ответ построен по предыдущему согласованному снимку тех же исходников",
        }
    }
}

/// Начало предупреждения о неполной близости чанков; по нему слои отличают такое предупреждение от прочих.
pub const SIMILARITY_WARNING_PREFIX: &str = "Близость посчитана для ";
const LEGACY_SIMILARITY_WARNING_PREFIX: &str = "Граф близости частичный: ";

/// «Близость посчитана для 539 из 1856 чанков: достигнут предел сравнений».
pub fn similarity_partial_warning(done: usize, total: usize, reason: &str) -> String {
    format!("{SIMILARITY_WARNING_PREFIX}{done} из {total} чанков: {reason}")
}

pub fn is_similarity_warning(warning: &str) -> bool {
    warning.starts_with(SIMILARITY_WARNING_PREFIX)
        || warning.starts_with(LEGACY_SIMILARITY_WARNING_PREFIX)
}

/// Предупреждение графа, построенного до ISS-83 («Граф близости частичный: Some(ComparisonLimit); завершено 539/1856 строк»),
/// читается человеческим текстом; остальные строки возвращаются как есть.
pub fn humanize_warning(warning: String) -> String {
    let Some(rest) = warning.strip_prefix(LEGACY_SIMILARITY_WARNING_PREFIX) else {
        return warning;
    };
    let Some((reason, counts)) = rest.split_once("; завершено ") else {
        return warning;
    };
    let reason = match reason {
        "Some(ComparisonLimit)" => "достигнут предел сравнений",
        "Some(Cancelled)" => "расчёт остановлен",
        _ => return warning,
    };
    let counts = counts.trim_end_matches(" строк");
    match counts.split_once('/') {
        Some((done, total)) => match (done.parse(), total.parse()) {
            (Ok(done), Ok(total)) => similarity_partial_warning(done, total, reason),
            _ => warning,
        },
        None => warning,
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    #[default]
    NotBuilt,
    Current,
    Stale,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Completeness {
    Complete,
    Partial,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    #[default]
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    #[default]
    Idle,
    Queued,
    Running,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerCoverage {
    pub unit: String,
    pub processed: usize,
    pub total: usize,
    pub missing: usize,
    pub comparisons: u64,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphStatus {
    pub freshness: Freshness,
    pub completeness: Completeness,
    pub availability: Availability,
    pub job_state: JobState,
    pub state: LayerState,
    pub snapshot: Option<String>,
    pub active_job: Option<String>,
    pub source_count: usize,
    pub error: Option<String>,
    pub method: String,
    #[serde(default)]
    pub progress: Option<GraphProgress>,
    #[serde(default)]
    pub file_errors: Vec<GraphFileError>,
    #[serde(default)]
    pub file_errors_total: usize,
    #[serde(default)]
    pub layers: BTreeMap<String, GraphLayerStatus>,
    #[serde(default)]
    pub paused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphProgress {
    pub phase: String,
    pub processed: usize,
    pub total: Option<usize>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphFileError {
    pub path: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphLayerStatus {
    #[serde(default)]
    pub stale_reason: Option<String>,
    #[serde(default)]
    pub freshness: Freshness,
    #[serde(default)]
    pub completeness: Completeness,
    #[serde(default)]
    pub availability: Availability,
    #[serde(default)]
    pub coverage: LayerCoverage,
    #[serde(default)]
    pub source_revisions: BTreeMap<String, String>,
    pub state: LayerState,
    pub generation: Option<String>,
    pub fingerprint: String,
    pub processed: usize,
    pub total: usize,
    pub failed: usize,
}

impl GraphStatus {
    pub fn derive_data_state(&mut self) {
        let layers: Vec<_> = self
            .layers
            .values()
            .filter(|l| l.availability == Availability::Enabled)
            .collect();
        self.availability = if !self.layers.is_empty() && layers.is_empty() {
            Availability::Disabled
        } else {
            Availability::Enabled
        };
        self.freshness = if layers.iter().any(|l| l.freshness == Freshness::Stale) {
            Freshness::Stale
        } else if layers.is_empty() || layers.iter().any(|l| l.freshness == Freshness::NotBuilt) {
            Freshness::NotBuilt
        } else {
            Freshness::Current
        };
        self.completeness = if layers
            .iter()
            .any(|l| l.completeness == Completeness::Partial)
        {
            Completeness::Partial
        } else if layers.is_empty()
            || layers.iter().any(|l| {
                l.completeness == Completeness::Unknown || l.freshness == Freshness::NotBuilt
            })
        {
            Completeness::Unknown
        } else {
            Completeness::Complete
        };
        self.state = if self.availability == Availability::Disabled {
            LayerState::Disabled
        } else {
            match self.freshness {
                Freshness::Stale => LayerState::Stale,
                Freshness::NotBuilt => LayerState::NotBuilt,
                Freshness::Current => LayerState::Ready,
            }
        };
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokenLink {
    pub source: String,
    pub evidence: String,
    pub target: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dataset {
    pub namespace: String,
    pub docs_root: String,
    pub snapshot: String,
    pub sources: Vec<SourceDocument>,
    #[serde(default)]
    pub failed_sources: Vec<SourceMeta>,
    #[serde(default)]
    pub layers: BTreeMap<String, GraphLayerStatus>,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub evidence: Vec<Evidence>,
    pub broken_links: Vec<BrokenLink>,
    pub semantic_state: LayerState,
    #[serde(default = "legacy_similarity_state")]
    pub similarity_state: LayerState,
    pub warnings: Vec<String>,
}

impl Dataset {
    pub fn empty(namespace: &str, docs_root: &str) -> Self {
        Self {
            namespace: namespace.into(),
            docs_root: docs_root.into(),
            snapshot: String::new(),
            sources: vec![],
            failed_sources: vec![],
            layers: BTreeMap::new(),
            nodes: vec![],
            edges: vec![],
            evidence: vec![],
            broken_links: vec![],
            semantic_state: LayerState::Partial,
            similarity_state: LayerState::NotBuilt,
            warnings: vec![],
        }
    }
    pub fn fingerprint(&mut self) {
        self.nodes
            .sort_by(|a, b| a.reference.id.cmp(&b.reference.id));
        self.edges.sort_by(|a, b| a.id.cmp(&b.id));
        self.evidence.sort_by(|a, b| a.id.cmp(&b.id));
        let mut sources: Vec<_> = self
            .sources
            .iter()
            .map(|s| (&s.meta.path, &s.meta.hash))
            .collect();
        sources.sort();
        let payload = serde_json::to_vec(&(
            &self.namespace,
            METHOD_VERSION,
            sources,
            &self.nodes,
            &self.edges,
            &self.failed_sources,
            self.semantic_state,
            self.similarity_state,
        ))
        .expect("граф сериализуется");
        self.snapshot = blake3::hash(&payload).to_hex().to_string();
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    #[serde(default)]
    pub modules: Vec<String>,
    #[serde(default)]
    pub files: Vec<String>,
}
/// Файл из области подходит документу в обеих формах пути: от корня документации и от корня
/// проекта (OQ-169).
pub(crate) fn file_matches(input: &str, source: &SourceMeta) -> bool {
    let normalized = crate::hints::normalize_file(input);
    normalized == source.docs_path || normalized == source.path
}

impl Scope {
    pub fn allows(&self, source: &SourceMeta) -> bool {
        (self.modules.is_empty() && self.files.is_empty())
            || self.modules.contains(&source.module)
            || self.files.iter().any(|p| file_matches(p, source))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Budgets {
    pub max_depth: Option<u32>,
    pub max_nodes: Option<u32>,
    pub max_edges: Option<u32>,
    pub max_fanout: Option<u32>,
    pub max_paths: Option<u32>,
    pub timeout_ms: Option<u32>,
    pub max_bytes: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppliedBudgets {
    pub max_depth: u32,
    pub max_nodes: u32,
    pub max_edges: u32,
    pub max_fanout: u32,
    pub max_paths: u32,
    pub timeout_ms: u32,
    pub max_bytes: u32,
}
impl Budgets {
    pub fn resolve(&self) -> Result<(AppliedBudgets, bool), GraphError> {
        let mut clamped = false;
        let mut apply = |name: &str, value: Option<u32>, default: u32, max: u32, zero: bool| {
            let value = value.unwrap_or(default);
            if value == 0 && !zero {
                return Err(GraphError::InvalidParams(format!(
                    "{name}: требуется положительное число"
                )));
            }
            clamped |= value > max;
            Ok(value.min(max))
        };
        let applied = AppliedBudgets {
            max_depth: apply("max_depth", self.max_depth, 2, 6, true)?,
            max_nodes: apply("max_nodes", self.max_nodes, 250, 2000, false)?,
            max_edges: apply("max_edges", self.max_edges, 1000, 10000, false)?,
            max_fanout: apply("max_fanout", self.max_fanout, 50, 200, false)?,
            max_paths: apply("max_paths", self.max_paths, 5, 20, false)?,
            timeout_ms: apply("timeout_ms", self.timeout_ms, 1000, 5000, false)?,
            max_bytes: apply("max_bytes", self.max_bytes, 262144, 2097152, false)?,
        };
        Ok((applied, clamped))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Relations,
    Paths,
    Impact,
    Context,
    Explore,
    #[default]
    Graph,
}
impl Operation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Relations => "связи",
            Self::Paths => "пути",
            Self::Impact => "влияние",
            Self::Context => "контекст",
            Self::Explore => "обзор",
            Self::Graph => "карта",
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum View {
    #[default]
    Documents,
    Sections,
    Chunks,
    Entities,
    Mixed,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Direction {
    Incoming,
    Outgoing,
    #[default]
    Both,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum PathKind {
    #[default]
    Semantic,
    Structural,
    Mixed,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Analysis {
    #[default]
    Topics,
    Connectivity,
    Coverage,
    ConflictCandidates,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Grouping {
    #[default]
    Communities,
    Modules,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum RetrievalMode {
    #[default]
    Hybrid,
    Mix,
    Graph,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Endpoint {
    Document {
        document: String,
    },
    Ref(ObjectRef),
    Name {
        label: String,
        kind: Option<Kind>,
        namespace: Option<String>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GraphRequest {
    pub operation: Operation,
    pub scope: Scope,
    pub budgets: Budgets,
    pub limit: Option<u32>,
    pub snapshot: Option<String>,
    pub cursor: Option<String>,
    pub query: Option<String>,
    pub entity_ref: Option<ObjectRef>,
    pub from: Option<Endpoint>,
    pub to: Option<Endpoint>,
    pub roots: Vec<ObjectRef>,
    pub documents: Vec<String>,
    pub predicates: Vec<Predicate>,
    pub direction: Direction,
    pub polarity: Option<Polarity>,
    pub qualifiers: BTreeMap<String, String>,
    pub view: View,
    pub local: bool,
    pub include_paths: bool,
    pub path_kind: PathKind,
    pub max_hops: Option<u32>,
    pub max_paths: Option<u32>,
    pub topic: Option<String>,
    pub whole_scope: bool,
    pub analysis: Analysis,
    pub grouping: Grouping,
    pub roles: Vec<Role>,
    pub task_profile: Option<TaskProfile>,
    pub token_budget: Option<u32>,
    pub tokenizer_id: Option<String>,
    pub change_description: Option<String>,
}

impl GraphRequest {
    pub fn context_roles(&self) -> Vec<Role> {
        let available = match self.task_profile {
            Some(TaskProfile::Investigation) => vec![
                Role::Behavior,
                Role::Architecture,
                Role::Constraint,
                Role::Verification,
            ],
            Some(TaskProfile::Decision) => {
                vec![Role::Requirement, Role::Decision, Role::Constraint]
            }
            _ => vec![
                Role::Requirement,
                Role::Decision,
                Role::Behavior,
                Role::Architecture,
                Role::Constraint,
                Role::Verification,
            ],
        };
        available
            .into_iter()
            .filter(|role| self.roles.is_empty() || self.roles.contains(role))
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Traversal {
    pub edge: String,
    pub reversed: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphPath {
    pub nodes: Vec<ObjectRef>,
    pub steps: Vec<Traversal>,
    pub evidence: Vec<String>,
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Item {
    Node {
        node: Node,
    },
    Edge {
        edge: Edge,
    },
    Relation {
        assertion: Edge,
    },
    Path {
        path: GraphPath,
    },
    Impact {
        node: ObjectRef,
        direct: bool,
        path: GraphPath,
    },
    RoleGroup {
        role: Role,
        evidence: Vec<String>,
    },
    Topic {
        id: String,
        label: String,
        members: Vec<ObjectRef>,
        representatives: Vec<ObjectRef>,
    },
    Audit {
        kind: String,
        node: Option<ObjectRef>,
        evidence: Vec<String>,
        detail: String,
    },
    Conflict {
        assertions: Vec<String>,
        evidence: Vec<String>,
        candidate: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Coverage {
    pub indexed: usize,
    pub processed: usize,
    pub failed: usize,
    pub denominator: Option<usize>,
    pub method: String,
    pub semantic_completeness: Option<f32>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphReply {
    #[serde(default)]
    pub layers: BTreeMap<String, GraphLayerStatus>,
    pub schema_version: u32,
    pub operation: Operation,
    pub scope: Scope,
    pub snapshot: String,
    pub generations: BTreeMap<String, String>,
    pub layer_state: LayerState,
    pub layer_states: BTreeMap<String, LayerState>,
    pub coverage: Coverage,
    pub requested: GraphRequest,
    pub applied: AppliedBudgets,
    pub applied_limit: u32,
    pub clamped: bool,
    pub items: Vec<Item>,
    pub nodes: Vec<Node>,
    pub sources: Vec<SourceMeta>,
    pub assertions: Vec<Edge>,
    pub evidence: Vec<Evidence>,
    pub truncated: bool,
    pub limit_reasons: Vec<String>,
    pub next_cursor: Option<String>,
    pub warnings: Vec<String>,
    pub method: String,
    pub elapsed_ms: u64,
    pub resolution: String,
    pub candidates: Vec<Node>,
    pub missing_roles: Vec<Role>,
    pub budget_mode: String,
    pub used_bytes: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("неверные параметры графа: {0}")]
    InvalidParams(String),
    #[error("ссылка на объект устарела: {0}")]
    StaleRef(String),
    #[error("снимок графа устарел")]
    SnapshotExpired,
    #[error("курсор не соответствует запросу")]
    CursorMismatch,
    #[error("граф ещё не готов: {0}")]
    NotReady(String),
    #[error("возможность графа недоступна: {0}")]
    CapabilityUnavailable(String),
    #[error("обработка графа отменена")]
    Cancelled,
    #[error("превышено время обработки графа")]
    TimedOut,
    #[error("ошибка графа: {0}")]
    InvalidData(String),
    #[cfg(feature = "storage")]
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
impl GraphError {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::InvalidParams(_) => "invalid_params",
            Self::StaleRef(_) => "stale_ref",
            Self::SnapshotExpired => "snapshot_expired",
            Self::CursorMismatch => "cursor_mismatch",
            Self::NotReady(_) => "graph_not_ready",
            Self::CapabilityUnavailable(_) => "graph_capability_unavailable",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timeout",
            _ => "internal",
        }
    }
}
