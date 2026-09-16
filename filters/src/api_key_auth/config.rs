// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Praxis Contributors

//! Configuration for the API key authentication filter.

use serde::Deserialize;

// -----------------------------------------------------------------------------
// ApiKeyAuthConfig
// -----------------------------------------------------------------------------

/// Deserialized YAML config for the API key auth filter.
///
/// ```yaml
/// filter: api_key_auth
/// validate_url: "http://maas-api:8080/internal/v1/api-keys/validate"
/// token_header: "x-api-key"
/// cache_ttl_seconds: 300
/// timeout_seconds: 5
/// ```
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ApiKeyAuthConfig {
    /// URL of the key validation endpoint.
    pub validate_url: String,

    /// Header to read the API key from.
    #[serde(default = "default_token_header")]
    pub token_header: String,

    /// How long to cache valid keys (seconds).
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl_seconds: u64,

    /// HTTP timeout for the validation callout (seconds).
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}

/// Default header for API key extraction.
fn default_token_header() -> String {
    "x-api-key".to_owned()
}

/// Default cache TTL: 5 minutes.
fn default_cache_ttl() -> u64 {
    300
}

/// Default callout timeout: 5 seconds.
fn default_timeout() -> u64 {
    5
}

// -----------------------------------------------------------------------------
// Validation
// -----------------------------------------------------------------------------

/// Validate the parsed configuration.
pub(super) fn validate_config(config: &ApiKeyAuthConfig) -> Result<(), String> {
    if config.validate_url.is_empty() {
        return Err("api_key_auth: validate_url must not be empty".into());
    }
    if config.timeout_seconds == 0 {
        return Err("api_key_auth: timeout_seconds must be greater than 0".into());
    }
    Ok(())
}
