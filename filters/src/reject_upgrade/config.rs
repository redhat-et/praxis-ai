// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 Praxis Contributors

//! Configuration for the reject-upgrade filter.

use serde::Deserialize;

// -----------------------------------------------------------------------------
// RejectUpgradeConfig
// -----------------------------------------------------------------------------

/// Deserialized YAML config for the reject-upgrade filter.
///
/// ```yaml
/// filter: reject_upgrade
/// # optional: restrict to specific upgrade tokens (default: reject all)
/// protocols:
///   - websocket
/// # optional: status + body returned on rejection (defaults below)
/// status: 400
/// message: "connection upgrade not supported on this route"
/// ```
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RejectUpgradeConfig {
    /// HTTP status returned when an upgrade attempt is refused.
    ///
    /// Must be a 4xx or 5xx code so upgrade-capable clients treat the route
    /// as having no `WebSocket` channel and fall back to plain HTTP.
    #[serde(default = "default_status")]
    pub status: u16,

    /// Upgrade protocol tokens to reject (case-insensitive, e.g. `websocket`).
    ///
    /// When empty, every request carrying an `Upgrade` header is rejected.
    #[serde(default)]
    pub protocols: Vec<String>,

    /// Response body returned with the rejection.
    #[serde(default = "default_message")]
    pub message: String,
}

/// Default rejection status (`400 Bad Request`).
fn default_status() -> u16 {
    400
}

/// Default rejection body.
fn default_message() -> String {
    "connection upgrade not supported on this route".to_owned()
}

// -----------------------------------------------------------------------------
// Validation
// -----------------------------------------------------------------------------

/// Validate a [`RejectUpgradeConfig`], returning an error on invalid values.
pub(super) fn validate_config(config: &RejectUpgradeConfig) -> Result<(), String> {
    if !(400..=599).contains(&config.status) {
        return Err(format!(
            "reject_upgrade: status must be a 4xx or 5xx code, got {}",
            config.status
        ));
    }
    if config.protocols.iter().any(|p| p.trim().is_empty()) {
        return Err("reject_upgrade: protocols entries must not be empty".into());
    }
    Ok(())
}
