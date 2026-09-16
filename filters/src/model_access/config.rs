// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Praxis Contributors

//! Configuration types for the model access filter.

use serde::Deserialize;

// -----------------------------------------------------------------------------
// Config
// -----------------------------------------------------------------------------

/// Deserialized YAML config for the model access filter.
///
/// ```yaml
/// filter: model_access
/// mode: denylist
/// models:
///   - "claude-fable-*"
///   - "o3-pro"
/// overrides:
///   - groups: ["ai-eng", "executive"]
///     mode: allowlist
///     models: ["*"]
/// ```
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ModelAccessConfig {
    /// Default access mode applied when no group override matches.
    pub mode: AccessMode,

    /// Default model patterns. Supports trailing `*` for prefix matching.
    pub models: Vec<String>,

    /// Per-group overrides. Checked in order; first matching group wins.
    #[serde(default)]
    pub overrides: Vec<GroupOverride>,

    /// Maximum request body bytes to buffer for model extraction.
    #[serde(default = "default_max_body_bytes")]
    pub max_body_bytes: usize,

    /// Metadata key for user's group (set by api_key_auth or jwt_auth).
    #[serde(default = "default_group_metadata_key")]
    pub group_metadata_key: String,
}

/// Per-group access override.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GroupOverride {
    /// Groups this override applies to.
    pub groups: Vec<String>,

    /// Access mode for these groups.
    pub mode: AccessMode,

    /// Model patterns for these groups.
    pub models: Vec<String>,
}

/// Access control mode.
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(super) enum AccessMode {
    /// Only listed models are allowed.
    Allowlist,
    /// Listed models are denied; everything else is allowed.
    Denylist,
}

fn default_max_body_bytes() -> usize {
    65_536
}

fn default_group_metadata_key() -> String {
    "x-tenant-group".to_owned()
}

/// Validate config.
pub(super) fn validate_config(cfg: &ModelAccessConfig) -> Result<(), String> {
    if cfg.models.is_empty() && cfg.overrides.is_empty() {
        return Err("at least one of models or overrides must be non-empty".to_owned());
    }
    for (i, o) in cfg.overrides.iter().enumerate() {
        if o.groups.is_empty() {
            return Err(format!("override[{}]: groups must not be empty", i));
        }
        if o.models.is_empty() {
            return Err(format!("override[{}]: models must not be empty", i));
        }
    }
    Ok(())
}
