// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 Praxis Contributors

//! Reject-upgrade filter: refuses HTTP/1.1 `Upgrade` requests (e.g. `WebSocket`)
//! on the chain it is installed in, so upgrade-capable clients fall back to
//! plain HTTP.
//!
//! Pingora detects an `Upgrade` request at its boundary and forwards it as an
//! opaque, bidirectional byte tunnel. The AI metering and token-counting
//! filters parse HTTP request/response bodies and SSE, so any traffic carried
//! inside an upgraded connection is never metered — it reaches the provider and
//! incurs cost, but produces no usage event, escapes quota enforcement, and is
//! invisible on the dashboard.
//!
//! Installing this filter on an LLM chain rejects the upgrade during the request
//! phase, before Pingora establishes the tunnel. Clients that default to a
//! `WebSocket` transport (e.g. `OpenCode`'s `OpenAI` provider) see a non-101
//! response and reconnect over HTTP, where every request is metered normally.

mod config;

#[cfg(test)]
#[expect(clippy::allow_attributes, reason = "blanket test suppressions")]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used, reason = "tests")]
mod tests;

use async_trait::async_trait;
use bytes::Bytes;
use http::header::UPGRADE;
use praxis_filter::{FilterAction, FilterError, HttpFilter, HttpFilterContext, Rejection, parse_filter_config};
use tracing::debug;

use self::config::{RejectUpgradeConfig, validate_config};

// -----------------------------------------------------------------------------
// RejectUpgradeFilter
// -----------------------------------------------------------------------------

/// Rejects connection-upgrade requests so they cannot bypass body-level filters.
///
/// Place it early in an LLM chain (before `external_metering`) so a `WebSocket`
/// handshake is refused before any upstream connection is made.
///
/// # YAML configuration
///
/// ```yaml
/// filter: reject_upgrade
/// protocols:
///   - websocket
/// ```
///
/// # Example
///
/// ```rust
/// use praxis_ai_filters::RejectUpgradeFilter;
/// use praxis_filter::HttpFilter;
///
/// let yaml: serde_yaml::Value = serde_yaml::from_str("protocols:\n  - websocket\n").unwrap();
/// let filter = RejectUpgradeFilter::from_config(&yaml).unwrap();
/// assert_eq!(filter.name(), "reject_upgrade");
/// ```
pub struct RejectUpgradeFilter {
    /// Status returned on rejection (validated to 4xx/5xx).
    status: u16,

    /// Lowercased upgrade tokens to reject; empty means reject any upgrade.
    protocols: Vec<String>,

    /// Rejection response body.
    message: Bytes,
}

impl RejectUpgradeFilter {
    /// Parse from YAML config.
    ///
    /// # Errors
    ///
    /// Returns [`FilterError`] if config parsing or validation fails.
    pub fn from_config(value: &serde_yaml::Value) -> Result<Box<dyn HttpFilter>, FilterError> {
        let config: RejectUpgradeConfig = parse_filter_config("reject_upgrade", value)?;
        validate_config(&config).map_err(|e| -> FilterError { e.into() })?;

        Ok(Box::new(Self {
            status: config.status,
            protocols: config.protocols.iter().map(|p| p.trim().to_ascii_lowercase()).collect(),
            message: Bytes::from(config.message.into_bytes()),
        }))
    }

    /// Whether the request's `Upgrade` header should be rejected.
    ///
    /// Rejects when an `Upgrade` header is present and either no protocol
    /// allow-list is configured (reject all upgrades) or one of the offered
    /// tokens matches the configured list. A non-UTF-8 token is rejected to
    /// fail closed.
    fn should_reject(&self, ctx: &HttpFilterContext<'_>) -> bool {
        let Some(value) = ctx.request.headers.get(UPGRADE) else {
            return false;
        };
        let Ok(raw) = value.to_str() else {
            // A non-UTF-8 upgrade token can't be classified; refuse it.
            return true;
        };
        if raw.trim().is_empty() {
            return false;
        }
        if self.protocols.is_empty() {
            return true;
        }
        // An Upgrade header may offer several comma-separated tokens, each
        // optionally carrying a version (`name/version`). Match on the name.
        raw.split(',').any(|token| {
            let token = token.trim().to_ascii_lowercase();
            let name = token.split('/').next().unwrap_or(token.as_str()).trim();
            self.protocols.iter().any(|p| p == name)
        })
    }
}

#[async_trait]
impl HttpFilter for RejectUpgradeFilter {
    fn name(&self) -> &'static str {
        "reject_upgrade"
    }

    async fn on_request(&self, ctx: &mut HttpFilterContext<'_>) -> Result<FilterAction, FilterError> {
        if self.should_reject(ctx) {
            debug!(status = self.status, "rejecting connection upgrade attempt");
            // Rejection defaults to closing the downstream connection, which is
            // correct here: the client sent an upgrade handshake, not a normal
            // request, so the connection must not be reused.
            return Ok(FilterAction::Reject(
                Rejection::status(self.status).with_body(self.message.clone()),
            ));
        }
        Ok(FilterAction::Continue)
    }
}
