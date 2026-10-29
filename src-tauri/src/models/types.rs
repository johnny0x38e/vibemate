//! Saved-model requests, records, and source states.
//! Failure codes are `crate::model_error::ModelError`.

use crate::model_catalog::{CapabilityOverrides, ModelCapabilities, ThinkingEffort};
use crate::model_identity::RouteSupport;
use crate::providers::ProviderProtocol;
use serde::{Deserialize, Serialize};

/// Largest page of models.
pub const MAX_MODEL_PAGE_SIZE: u32 = 200;

/// Most model IDs accepted by one selection change.
pub const MAX_SELECTION_BATCH: usize = 500;

/// Where a model row came from first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelSource {
    Fetched,
    Manual,
}

impl ModelSource {
    /// Encode the stored source using its existing SQLite representation.
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Fetched => "fetched",
            Self::Manual => "manual",
        }
    }

    /// Reject source values this build cannot interpret.
    pub(super) fn from_value(value: &str) -> Option<Self> {
        match value {
            "fetched" => Some(Self::Fetched),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }
}

/// Whether the vendor's latest complete list contained the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamState {
    /// The latest complete fetch listed it.
    Listed,
    /// An earlier fetch listed it, the latest complete fetch did not.
    Missing,
    /// No fetch has listed it (only manual rows).
    NeverListed,
}

/// One model as shown to the frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModel {
    pub provider_id: String,
    pub model_id: String,
    pub source: ModelSource,
    pub selected: bool,
    pub alias: Option<String>,
    pub upstream_name: Option<String>,
    pub context_window: Option<i64>,
    pub max_output_tokens: Option<i64>,
    pub input_modalities: Option<Vec<String>>,
    pub output_modalities: Option<Vec<String>>,
    pub supported_endpoints: Option<Vec<String>>,
    pub capabilities: ModelCapabilities,
    /// User capability declarations; `None` fields follow `capabilities`.
    pub capability_overrides: CapabilityOverrides,
    /// Stated thinking-effort options. `None` means no statement yet.
    pub thinking_effort: Option<ThinkingEffort>,
    pub route_support: RouteSupport,
    /// Protocol to use for this model on the instance (P15). `None` when Command
    /// Code has no usable route declaration.
    pub resolved_protocol: Option<ProviderProtocol>,
    pub upstream_state: UpstreamState,
    pub last_seen_at_ms: Option<i64>,
    pub missing_since_ms: Option<i64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// A page request. `filter` is `"all"` or `"selected"`.
///
/// Without a `query` (or one that is empty after normalization) the list is
/// ordered by model ID and paged with `after`. With a query the best `limit`
/// matches are returned in rank order, with no cursor; `after` must then be `None`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListProviderModelsRequest {
    pub provider_id: String,
    /// The `next_cursor` of the previous page, or `None` for the first page.
    pub after: Option<String>,
    /// Page size (or, with a query, result count) from 1 to `MAX_MODEL_PAGE_SIZE`.
    pub limit: u32,
    pub filter: String,
    /// Fuzzy search text, at most `MAX_QUERY_CHARS` characters.
    #[serde(default)]
    pub query: Option<String>,
}

/// One page of models.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelPage {
    pub items: Vec<ProviderModel>,
    /// Cursor for the next page; always `None` for a search.
    pub next_cursor: Option<String>,
    /// For a search, how many models matched (`items` holds at most `limit`);
    /// `None` without a query.
    pub total_matches: Option<usize>,
}

/// How many models the user has added across every provider (`selected = 1`).
/// Unselected catalog-fetch rows are excluded; this is not a paged list total.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelCount {
    pub total: i64,
}

/// Select or unselect several models at once.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetModelsSelectedRequest {
    pub provider_id: String,
    pub model_ids: Vec<String>,
    pub selected: bool,
}

/// Add a model by hand.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddManualModelRequest {
    pub provider_id: String,
    pub model_id: String,
    pub alias: Option<String>,
    /// Capability declarations chosen in the create form. The field defaults to
    /// "no declarations", so every capability follows the (empty) catalog result.
    #[serde(default)]
    pub capability_overrides: CapabilityOverrides,
    /// Context window in tokens. `None` stores no limit.
    pub context_window: Option<i64>,
    /// Maximum output in tokens. `None` stores no limit.
    pub max_output_tokens: Option<i64>,
    /// Thinking-effort statement. `None` stores no statement.
    pub thinking_effort: Option<ThinkingEffort>,
}

/// Delete a model that was added by hand.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteManualModelRequest {
    pub provider_id: String,
    pub model_id: String,
}

/// Agent bindings and remembered choices that a model deletion would affect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDeletionImpact {
    /// Provider that owns the model row.
    pub provider_id: String,
    /// Model ID the dialog is about.
    pub model_id: String,
    /// Grok aliases and live Pi entries that still name this model.
    pub references: Vec<crate::references::AgentReference>,
    /// The owned Codex default when it names this Provider/model pair.
    pub codex_references: Vec<crate::references::CodexReference>,
    /// Claude Code remembered choices removed by the foreign-key cascade.
    pub remembered_preferences: u64,
    /// True only when no binding row blocks deletion.
    pub can_delete: bool,
}

/// Remove one stale Grok alias binding for a Provider/model pair.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClearModelReferenceRequest {
    pub provider_id: String,
    pub model_id: String,
    /// Currently only `grokBuild` has a clearable model binding.
    pub agent_id: String,
    pub config_root: String,
    pub label: String,
}

/// Upstream metadata for a model the user wants to keep selected (P12.c.5).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveProviderModelEntry {
    pub model_id: String,
    pub upstream_name: Option<String>,
    pub context_window: Option<i64>,
    pub max_output_tokens: Option<i64>,
    pub input_modalities: Option<Vec<String>>,
    pub output_modalities: Option<Vec<String>>,
    pub supported_endpoints: Option<Vec<String>>,
    #[serde(default)]
    pub capabilities: ModelCapabilities,
    /// Thinking-effort statement from the catalog page. `None` does not clear a
    /// value already stored on an existing row.
    pub thinking_effort: Option<ThinkingEffort>,
}

/// Atomically remove deselected models and upsert newly selected ones.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveProviderModelSelectionsRequest {
    pub provider_id: String,
    pub remove_model_ids: Vec<String>,
    pub add: Vec<SaveProviderModelEntry>,
}
