//! Local model selection and manual editing, each committed as one SQLite transaction.

use super::repository::{
    lock, prepare_model_parameters, read_model, read_provider, stored_overrides,
};
use super::types::*;
use super::{ModelError, parse_provider_id};
use crate::model_catalog::{ModelCapabilities, accepted_limit, encode_thinking_effort};
use crate::model_identity::{RouteSupport, is_valid_model_id, route_support};
use crate::providers::validate_display_name;
use crate::references::{self, ReferenceClearError, ReferenceReadError};
use crate::storage::Storage;
use rusqlite::{TransactionBehavior, params};
use std::collections::BTreeSet;

/// Select or unselect models in one transaction: either every listed model
/// changes or none does. Returns the changed models ordered by model ID.
///
/// # Errors
/// `InvalidRequest` for an empty or oversized batch or a malformed ID;
/// `NotFound` for an unknown provider; `ModelNotFound` when any model is missing;
/// `ModelRouteNotSupported` when selecting a fetched model whose routes do not
/// include the instance's protocol.
pub fn set_models_selected(
    storage: &Storage,
    request: &SetModelsSelectedRequest,
    now_ms: i64,
) -> Result<Vec<ProviderModel>, ModelError> {
    let provider_id = parse_provider_id(&request.provider_id)?;
    let model_ids: BTreeSet<&str> = request.model_ids.iter().map(String::as_str).collect();
    if model_ids.is_empty()
        || request.model_ids.len() > MAX_SELECTION_BATCH
        || !model_ids.iter().all(|id| is_valid_model_id(id))
    {
        return Err(ModelError::InvalidRequest);
    }

    let mut connection = lock(storage)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| ModelError::WriteFailed)?;
    let provider = read_provider(&transaction, &provider_id)?;
    let mut changed = Vec::with_capacity(model_ids.len());
    for model_id in model_ids {
        let model = read_model(&transaction, &provider_id, model_id)?
            .ok_or(ModelError::ModelNotFound)?
            .into_model(provider)?;
        let blocked = request.selected
            && model.source == ModelSource::Fetched
            && !matches!(
                model.route_support,
                RouteSupport::Supported | RouteSupport::NotApplicable
            );
        if blocked {
            return Err(ModelError::ModelRouteNotSupported);
        }
        if model.selected != request.selected {
            transaction
                .execute(
                    "UPDATE provider_model SET selected = ?3, updated_at = max(updated_at, ?4)
                     WHERE provider_id = ?1 AND model_id = ?2",
                    params![provider_id.as_str(), model_id, request.selected, now_ms],
                )
                .map_err(|_| ModelError::WriteFailed)?;
        }
        let updated = read_model(&transaction, &provider_id, model_id)?
            .ok_or(ModelError::ModelNotFound)?
            .into_model(provider)?;
        changed.push(updated);
    }
    transaction.commit().map_err(|_| ModelError::WriteFailed)?;
    Ok(changed)
}

/// Add a model by hand. It is saved as `manual` and selected.
///
/// # Errors
/// `ModelIdInvalid`, `ModelAliasInvalid`, `NotFound`, `ModelAlreadyExists`, or a
/// storage error.
pub fn add_manual_model(
    storage: &Storage,
    request: &AddManualModelRequest,
    now_ms: i64,
) -> Result<ProviderModel, ModelError> {
    let provider_id = parse_provider_id(&request.provider_id)?;
    let model_id = request.model_id.trim();
    if !is_valid_model_id(model_id) {
        return Err(ModelError::ModelIdInvalid);
    }
    let alias = request
        .alias
        .as_deref()
        .map(|alias| validate_display_name(alias).map_err(|_| ModelError::ModelAliasInvalid))
        .transpose()?;
    let prepared = prepare_model_parameters(
        request.context_window,
        request.max_output_tokens,
        request.thinking_effort.clone(),
    )?;
    let context_window = prepared.context_window;
    let max_output_tokens = prepared.max_output_tokens;
    let thinking_effort = prepared.thinking_effort;

    let mut connection = lock(storage)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| ModelError::WriteFailed)?;
    let provider = read_provider(&transaction, &provider_id)?;
    if read_model(&transaction, &provider_id, model_id)?.is_some() {
        return Err(ModelError::ModelAlreadyExists);
    }
    transaction
        .execute(
            "INSERT INTO provider_model
                 (provider_id, model_id, source, selected, alias, context_window,
                  max_output_tokens, thinking_effort, capabilities,
                  capability_overrides, created_at, updated_at)
             VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
            params![
                provider_id.as_str(),
                model_id,
                ModelSource::Manual.as_str(),
                alias,
                context_window,
                max_output_tokens,
                thinking_effort,
                serde_json::to_string(&ModelCapabilities::unknown())
                    .map_err(|_| ModelError::WriteFailed)?,
                stored_overrides(&request.capability_overrides)?,
                now_ms
            ],
        )
        .map_err(|_| ModelError::WriteFailed)?;
    let model = read_model(&transaction, &provider_id, model_id)?
        .ok_or(ModelError::WriteFailed)?
        .into_model(provider)?;
    transaction.commit().map_err(|_| ModelError::WriteFailed)?;
    Ok(model)
}

/// List the Grok aliases and live Pi entries that a model deletion affects.
///
/// # Errors
/// `InvalidRequest` for malformed IDs, `NotFound` for an unknown provider,
/// `ModelNotFound` for an unknown row, and storage or read failures.
pub fn model_deletion_impact(
    storage: &Storage,
    provider_id: &str,
    model_id: &str,
) -> Result<ModelDeletionImpact, ModelError> {
    let provider_id = parse_provider_id(provider_id)?;
    if !is_valid_model_id(model_id) {
        return Err(ModelError::InvalidRequest);
    }
    {
        let connection = lock(storage)?;
        read_provider(&connection, &provider_id)?;
        read_model(&connection, &provider_id, model_id)?.ok_or(ModelError::ModelNotFound)?;
    }
    let references = references::model_references(storage, provider_id.as_str(), model_id)
        .map_err(map_reference_read)?;
    let remembered_preferences =
        references::remembered_preferences(storage, provider_id.as_str(), Some(model_id))
            .map_err(map_reference_read)?;
    let codex_references =
        references::codex_references(storage, provider_id.as_str(), Some(model_id))
            .map_err(map_reference_read)?;
    // The count repeats every blocking table as a safety net.
    let bound = references::model_has_bindings(storage, provider_id.as_str(), model_id)
        .map_err(map_reference_read)?;
    Ok(ModelDeletionImpact {
        provider_id: provider_id.as_str().to_owned(),
        model_id: model_id.to_owned(),
        can_delete: references.is_empty() && codex_references.is_empty() && !bound,
        references,
        codex_references,
        remembered_preferences,
    })
}

/// Remove one stale Grok alias binding for a Provider/model pair.
///
/// The Agent file must already be free of the alias. A live Pi model entry has
/// no central row and is removed by changing or restoring the Pi configuration.
///
/// # Errors
/// `InvalidRequest` for malformed fields, `NotFound` when the binding is gone,
/// `ModelInUse` when the Agent file still names it or cannot be read, and
/// storage failures.
pub fn clear_model_reference(
    storage: &Storage,
    request: ClearModelReferenceRequest,
) -> Result<(), ModelError> {
    let provider_id = parse_provider_id(&request.provider_id)?;
    if !is_valid_model_id(&request.model_id)
        || request.agent_id != "grokBuild"
        || !valid_reference_text(&request.config_root, 4096)
        || !valid_reference_text(&request.label, 4096)
    {
        return Err(ModelError::InvalidRequest);
    }
    references::clear_model_reference(
        storage,
        provider_id.as_str(),
        &request.model_id,
        &request.agent_id,
        &request.config_root,
        &request.label,
    )
    .map_err(map_reference_clear)
}

fn any_model_reference_blocks(
    storage: &Storage,
    provider_id: &str,
    model_id: &str,
) -> Result<bool, ModelError> {
    let references =
        references::model_references(storage, provider_id, model_id).map_err(map_reference_read)?;
    if references
        .iter()
        .any(|reference| reference.state != references::ReferenceState::Stale)
    {
        return Ok(true);
    }
    references::model_has_bindings(storage, provider_id, model_id).map_err(map_reference_read)
}

fn valid_reference_text(value: &str, limit: usize) -> bool {
    !value.is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}

fn map_reference_read(error: ReferenceReadError) -> ModelError {
    match error {
        ReferenceReadError::StorageUnavailable => ModelError::StorageUnavailable,
        ReferenceReadError::ReadFailed => ModelError::ReadFailed,
    }
}

fn map_reference_clear(error: ReferenceClearError) -> ModelError {
    match error {
        ReferenceClearError::StorageUnavailable => ModelError::StorageUnavailable,
        ReferenceClearError::ReadFailed => ModelError::ReadFailed,
        ReferenceClearError::WriteFailed => ModelError::WriteFailed,
        ReferenceClearError::InvalidRequest => ModelError::InvalidRequest,
        ReferenceClearError::NotFound => ModelError::NotFound,
        ReferenceClearError::InUse => ModelError::ModelInUse,
    }
}

/// Delete a model that was added by hand.
///
/// # Errors
/// `ModelNotFound` when no such model exists; `InvalidRequest` when the model was
/// fetched (fetched rows follow the vendor's list and are never deleted by hand);
/// `ModelInUse` while an Agent binding or live Pi entry still names it.
pub fn delete_manual_model(
    storage: &Storage,
    request: &DeleteManualModelRequest,
) -> Result<(), ModelError> {
    let provider_id = parse_provider_id(&request.provider_id)?;
    if !is_valid_model_id(&request.model_id) {
        return Err(ModelError::InvalidRequest);
    }
    if any_model_reference_blocks(storage, provider_id.as_str(), &request.model_id)? {
        return Err(ModelError::ModelInUse);
    }
    let mut connection = lock(storage)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| ModelError::WriteFailed)?;
    read_provider(&transaction, &provider_id)?;
    let stored = read_model(&transaction, &provider_id, &request.model_id)?
        .ok_or(ModelError::ModelNotFound)?;
    if stored.source != ModelSource::Manual.as_str() {
        return Err(ModelError::InvalidRequest);
    }
    let bindings: i64 = transaction
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM grok_model_binding
                 WHERE provider_id = ?1 AND model_id = ?2) +
                (SELECT COUNT(*) FROM codex_default_binding
                 WHERE provider_id = ?1 AND model_id = ?2)",
            params![provider_id.as_str(), request.model_id],
            |row| row.get(0),
        )
        .map_err(|_| ModelError::ReadFailed)?;
    if bindings > 0 {
        return Err(ModelError::ModelInUse);
    }
    transaction
        .execute(
            "DELETE FROM provider_model WHERE provider_id = ?1 AND model_id = ?2",
            params![provider_id.as_str(), request.model_id],
        )
        .map_err(|_| ModelError::WriteFailed)?;
    transaction.commit().map_err(|_| ModelError::WriteFailed)
}

/// Apply a Save on the Models tab: delete unchecked rows and upsert checked ones.
///
/// Only rows the user keeps selected remain in `provider_model`. Manual rows follow
/// the same rule when the user removes them from the selected list.
///
/// # Errors
/// `InvalidRequest` for malformed IDs, overlapping remove/add sets, or oversized
/// batches; `ModelRouteNotSupported` when an added fetched model does not support
/// the instance protocol; storage errors otherwise.
pub fn save_provider_model_selections(
    storage: &Storage,
    request: &SaveProviderModelSelectionsRequest,
    now_ms: i64,
) -> Result<(), ModelError> {
    let provider_id = parse_provider_id(&request.provider_id)?;
    let remove: BTreeSet<&str> = request
        .remove_model_ids
        .iter()
        .map(String::as_str)
        .collect();
    if remove.len() != request.remove_model_ids.len()
        || remove.len() > MAX_SELECTION_BATCH
        || !remove.iter().all(|id| is_valid_model_id(id))
    {
        return Err(ModelError::InvalidRequest);
    }
    if request.add.len() > MAX_SELECTION_BATCH {
        return Err(ModelError::InvalidRequest);
    }
    let mut add_ids = BTreeSet::new();
    for entry in &request.add {
        if !is_valid_model_id(&entry.model_id) || !add_ids.insert(entry.model_id.as_str()) {
            return Err(ModelError::InvalidRequest);
        }
        if remove.contains(entry.model_id.as_str()) {
            return Err(ModelError::InvalidRequest);
        }
    }
    if remove.is_empty() && request.add.is_empty() {
        return Err(ModelError::InvalidRequest);
    }
    for model_id in &remove {
        if any_model_reference_blocks(storage, provider_id.as_str(), model_id)? {
            return Err(ModelError::ModelInUse);
        }
    }

    let mut connection = lock(storage)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| ModelError::WriteFailed)?;
    let provider = read_provider(&transaction, &provider_id)?;
    for model_id in &remove {
        let bindings: i64 = transaction
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM grok_model_binding
                     WHERE provider_id = ?1 AND model_id = ?2) +
                    (SELECT COUNT(*) FROM codex_default_binding
                     WHERE provider_id = ?1 AND model_id = ?2)",
                params![provider_id.as_str(), model_id],
                |row| row.get(0),
            )
            .map_err(|_| ModelError::ReadFailed)?;
        if bindings > 0 {
            return Err(ModelError::ModelInUse);
        }
    }

    for model_id in remove {
        transaction
            .execute(
                "DELETE FROM provider_model WHERE provider_id = ?1 AND model_id = ?2",
                params![provider_id.as_str(), model_id],
            )
            .map_err(|_| ModelError::WriteFailed)?;
    }

    for entry in &request.add {
        let endpoints = json_column(&entry.supported_endpoints)?;
        let route = route_support(provider.kind, entry.supported_endpoints.as_deref());
        if !matches!(route, RouteSupport::Supported | RouteSupport::NotApplicable) {
            return Err(ModelError::ModelRouteNotSupported);
        }
        let input = json_column(&entry.input_modalities)?;
        let output = json_column(&entry.output_modalities)?;
        let capabilities =
            serde_json::to_string(&entry.capabilities).map_err(|_| ModelError::WriteFailed)?;
        // A missing catalog field must not wipe a limit the user already saved.
        let context_window =
            accepted_limit(entry.context_window).ok_or(ModelError::ModelParametersInvalid)?;
        let max_output_tokens =
            accepted_limit(entry.max_output_tokens).ok_or(ModelError::ModelParametersInvalid)?;
        let thinking_effort = match entry.thinking_effort.clone() {
            None => None,
            Some(effort) => Some(
                effort
                    .canonical()
                    .ok_or(ModelError::ModelParametersInvalid)?,
            ),
        };
        let thinking_effort = encode_thinking_effort(thinking_effort.as_ref())
            .map_err(|_| ModelError::WriteFailed)?;
        let existing = read_model(&transaction, &provider_id, &entry.model_id)?;
        if let Some(stored) = existing {
            if stored.source == ModelSource::Manual.as_str() {
                transaction
                    .execute(
                        "UPDATE provider_model SET
                             selected = 1, upstream_name = ?3,
                             context_window = COALESCE(?4, context_window),
                             max_output_tokens = COALESCE(?5, max_output_tokens),
                             input_modalities = ?6,
                             output_modalities = ?7, supported_endpoints = ?8, capabilities = ?9,
                             thinking_effort = COALESCE(?10, thinking_effort),
                             last_seen_at = ?11, missing_since = NULL,
                             updated_at = max(updated_at, ?11)
                         WHERE provider_id = ?1 AND model_id = ?2",
                        params![
                            provider_id.as_str(),
                            entry.model_id,
                            entry.upstream_name,
                            context_window,
                            max_output_tokens,
                            input,
                            output,
                            endpoints,
                            capabilities,
                            thinking_effort,
                            now_ms
                        ],
                    )
                    .map_err(|_| ModelError::WriteFailed)?;
            } else {
                transaction
                    .execute(
                        "UPDATE provider_model SET
                             selected = 1, upstream_name = ?3,
                             context_window = COALESCE(?4, context_window),
                             max_output_tokens = COALESCE(?5, max_output_tokens),
                             input_modalities = ?6,
                             output_modalities = ?7, supported_endpoints = ?8, capabilities = ?9,
                             thinking_effort = COALESCE(?10, thinking_effort),
                             last_seen_at = ?11, missing_since = NULL,
                             updated_at = max(updated_at, ?11)
                         WHERE provider_id = ?1 AND model_id = ?2",
                        params![
                            provider_id.as_str(),
                            entry.model_id,
                            entry.upstream_name,
                            context_window,
                            max_output_tokens,
                            input,
                            output,
                            endpoints,
                            capabilities,
                            thinking_effort,
                            now_ms
                        ],
                    )
                    .map_err(|_| ModelError::WriteFailed)?;
            }
        } else {
            transaction
                .execute(
                    "INSERT INTO provider_model
                         (provider_id, model_id, source, selected, upstream_name, context_window,
                          max_output_tokens, input_modalities, output_modalities,
                          supported_endpoints, capabilities, thinking_effort,
                          created_at, updated_at, last_seen_at)
                     VALUES (?1, ?2, 'fetched', 1, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11, ?11)",
                    params![
                        provider_id.as_str(),
                        entry.model_id,
                        entry.upstream_name,
                        context_window,
                        max_output_tokens,
                        input,
                        output,
                        endpoints,
                        capabilities,
                        thinking_effort,
                        now_ms
                    ],
                )
                .map_err(|_| ModelError::WriteFailed)?;
        }
    }

    transaction.commit().map_err(|_| ModelError::WriteFailed)
}

fn json_column(items: &Option<Vec<String>>) -> Result<Option<String>, ModelError> {
    items
        .as_ref()
        .map(|values| serde_json::to_string(values).map_err(|_| ModelError::WriteFailed))
        .transpose()
}
