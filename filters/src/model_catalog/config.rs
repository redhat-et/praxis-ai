// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Praxis Contributors

//! Configuration types for the model catalog filter.

use serde::Deserialize;

// -----------------------------------------------------------------------------
// Config
// -----------------------------------------------------------------------------

/// Deserialized YAML config for the model catalog filter.
///
/// ```yaml
/// filter: model_catalog
/// format: openai
/// path: /v1/models
/// models:
///   - id: llama-3.1-8b-instruct
///     owned_by: praxis
///   - id: claude-internal
///     display_name: "Claude (internal)"
/// ```
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ModelCatalogConfig {
    /// Response envelope dialect: `openai` or `anthropic`.
    #[serde(default)]
    pub format: CatalogFormat,

    /// Request path served by the catalog, matched exactly on `GET`.
    #[serde(default = "default_path")]
    pub path: String,

    /// Advertised models, in declaration order.
    pub models: Vec<ModelEntry>,
}

/// A single advertised model.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ModelEntry {
    /// Model identifier returned to clients and used in requests.
    pub id: String,

    /// Owner shown in the OpenAI envelope's `owned_by` field.
    #[serde(default = "default_owned_by")]
    pub owned_by: String,

    /// Human-readable name shown in the Anthropic envelope's
    /// `display_name` field. Defaults to [`ModelEntry::id`] when absent.
    #[serde(default)]
    pub display_name: Option<String>,

    /// Creation time as a Unix timestamp (seconds). Emitted verbatim by
    /// OpenAI (`created`) and formatted as RFC 3339 by Anthropic
    /// (`created_at`). Defaults to the epoch so the catalog stays
    /// deterministic without reading a clock.
    #[serde(default)]
    pub created: i64,
}

/// Response envelope dialect.
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub(super) enum CatalogFormat {
    /// OpenAI `GET /v1/models` list envelope.
    #[default]
    Openai,
    /// Anthropic `GET /v1/models` list envelope.
    Anthropic,
}

/// Default request path served by the catalog.
fn default_path() -> String {
    "/v1/models".to_owned()
}

/// Default owner label for a model with none configured.
fn default_owned_by() -> String {
    "praxis".to_owned()
}

/// Validate config beyond what deserialization guarantees.
pub(super) fn validate_config(cfg: &ModelCatalogConfig) -> Result<(), String> {
    if cfg.models.is_empty() {
        return Err("models must not be empty".to_owned());
    }
    if !cfg.path.starts_with('/') {
        return Err(format!("path must start with '/', got '{}'", cfg.path));
    }
    for (i, m) in cfg.models.iter().enumerate() {
        if m.id.is_empty() {
            return Err(format!("models[{i}]: id must not be empty"));
        }
    }
    Ok(())
}
