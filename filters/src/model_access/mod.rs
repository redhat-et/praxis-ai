// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Praxis Contributors

//! Model access control filter: enforces per-group allowlists or
//! denylists of model names by reading the `model` field from the
//! JSON request body and the user's group from `filter_metadata`.
//!
//! Supports glob patterns with a trailing `*` for prefix matching.
//!
//! ```yaml
//! filter: model_access
//! mode: denylist
//! models:
//!   - "claude-fable-*"
//!   - "o3-pro"
//! overrides:
//!   - groups: ["ai-eng"]
//!     mode: allowlist
//!     models: ["*"]
//! ```

mod config;

#[cfg(test)]
#[expect(clippy::allow_attributes, reason = "blanket test suppressions")]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used, reason = "tests")]
mod tests;

use async_trait::async_trait;
use bytes::Bytes;
use praxis_filter::{
    BodyAccess, BodyMode, FilterAction, FilterError, HttpFilter, HttpFilterContext, Rejection, parse_filter_config,
};
use serde::Deserialize;
use tracing::debug;

use self::config::{AccessMode, GroupOverride, ModelAccessConfig, validate_config};

// -----------------------------------------------------------------------------
// ModelPattern
// -----------------------------------------------------------------------------

/// A compiled model pattern — either exact match or prefix match.
enum ModelPattern {
    /// Matches any model.
    Wildcard,
    /// Matches a specific model name exactly.
    Exact(String),
    /// Matches models starting with a prefix.
    Prefix(String),
}

impl ModelPattern {
    fn from_str(s: &str) -> Self {
        if s == "*" {
            Self::Wildcard
        } else if let Some(prefix) = s.strip_suffix('*') {
            Self::Prefix(prefix.to_owned())
        } else {
            Self::Exact(s.to_owned())
        }
    }

    fn matches(&self, model: &str) -> bool {
        match self {
            Self::Wildcard => true,
            Self::Exact(name) => model == name,
            Self::Prefix(prefix) => model.starts_with(prefix.as_str()),
        }
    }
}

// -----------------------------------------------------------------------------
// AccessRule
// -----------------------------------------------------------------------------

/// A compiled access rule (default or per-group override).
struct AccessRule {
    mode: AccessMode,
    patterns: Vec<ModelPattern>,
}

impl AccessRule {
    fn is_allowed(&self, model: &str) -> bool {
        let matched = self.patterns.iter().any(|p| p.matches(model));
        match self.mode {
            AccessMode::Allowlist => matched,
            AccessMode::Denylist => !matched,
        }
    }
}

// -----------------------------------------------------------------------------
// GroupRule
// -----------------------------------------------------------------------------

/// A compiled group override with its group membership check.
struct GroupRule {
    groups: Vec<String>,
    rule: AccessRule,
}

// -----------------------------------------------------------------------------
// ModelAccessFilter
// -----------------------------------------------------------------------------

/// Enforces model access control by checking the `model` field in
/// the JSON request body against configured rules.
///
/// Reads the user's group from `filter_metadata` (set by upstream
/// auth filters like `api_key_auth` or `jwt_auth`) to apply
/// per-group overrides.
///
/// # YAML configuration
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
/// group_metadata_key: "x-tenant-group"
/// max_body_bytes: 65536
/// ```
///
/// # Example
///
/// ```rust
/// use praxis_ai_filters::ModelAccessFilter;
///
/// let yaml: serde_yaml::Value = serde_yaml::from_str(
///     r#"
/// mode: denylist
/// models:
///   - "test-model"
/// "#,
/// )
/// .unwrap();
/// let filter = ModelAccessFilter::from_config(&yaml).unwrap();
/// assert_eq!(filter.name(), "model_access");
/// ```
pub struct ModelAccessFilter {
    /// Default rule when no group override matches.
    default_rule: AccessRule,

    /// Per-group overrides, checked in order.
    group_rules: Vec<GroupRule>,

    /// Metadata key for the user's group.
    group_metadata_key: String,

    /// Maximum body bytes to buffer.
    max_body_bytes: usize,
}

impl ModelAccessFilter {
    /// Parse from YAML config.
    ///
    /// # Errors
    ///
    /// Returns [`FilterError`] if config parsing or validation fails.
    pub fn from_config(value: &serde_yaml::Value) -> Result<Box<dyn HttpFilter>, FilterError> {
        let cfg: ModelAccessConfig = parse_filter_config("model_access", value)?;
        validate_config(&cfg).map_err(|e| -> FilterError { e.into() })?;

        let default_rule = AccessRule {
            mode: cfg.mode,
            patterns: cfg.models.iter().map(|s| ModelPattern::from_str(s)).collect(),
        };

        let group_rules = cfg
            .overrides
            .into_iter()
            .map(|o: GroupOverride| GroupRule {
                groups: o.groups,
                rule: AccessRule {
                    mode: o.mode,
                    patterns: o.models.iter().map(|s| ModelPattern::from_str(s)).collect(),
                },
            })
            .collect();

        Ok(Box::new(Self {
            default_rule,
            group_rules,
            group_metadata_key: cfg.group_metadata_key,
            max_body_bytes: cfg.max_body_bytes,
        }))
    }

    /// Find the applicable rule for the user's group.
    fn find_rule(&self, group: Option<&str>) -> &AccessRule {
        if let Some(g) = group {
            for gr in &self.group_rules {
                if gr.groups.iter().any(|allowed| allowed == g) {
                    return &gr.rule;
                }
            }
        }
        &self.default_rule
    }
}

/// Minimal struct to extract only the `model` field from request JSON.
#[derive(Deserialize)]
struct ModelField {
    model: Option<String>,
}

#[async_trait]
impl HttpFilter for ModelAccessFilter {
    fn name(&self) -> &'static str {
        "model_access"
    }

    fn request_body_access(&self) -> BodyAccess {
        BodyAccess::ReadOnly
    }

    fn request_body_mode(&self) -> BodyMode {
        BodyMode::StreamBuffer {
            max_bytes: Some(self.max_body_bytes),
        }
    }

    async fn on_request(&self, ctx: &mut HttpFilterContext<'_>) -> Result<FilterAction, FilterError> {
        // Body is pre-read via StreamBuffer and available in
        // buffered_request_body during the header phase — before
        // on_request_body runs. This lets us read both the model
        // (from body) and the group (from filter_metadata, set by
        // api_key_auth earlier in the pipeline).
        let Some(data) = &ctx.buffered_request_body else {
            return Ok(FilterAction::Continue);
        };

        let model = match serde_json::from_slice::<ModelField>(data) {
            Ok(parsed) => parsed.model,
            Err(_) => {
                debug!("model_access: failed to parse request body, allowing");
                return Ok(FilterAction::Continue);
            },
        };

        let Some(model_name) = model else {
            debug!("model_access: no model field in request body, allowing");
            return Ok(FilterAction::Continue);
        };

        let group = ctx.filter_metadata.get(&self.group_metadata_key).map(|s| s.as_str());
        let rule = self.find_rule(group);

        if rule.is_allowed(&model_name) {
            debug!(model = %model_name, group = ?group, "model_access: allowed");
            Ok(FilterAction::Continue)
        } else {
            debug!(model = %model_name, group = ?group, "model_access: denied");
            let msg = format!("model '{}' is not permitted by access policy", model_name);
            Ok(FilterAction::Reject(Rejection {
                status: 403,
                body: Some(Bytes::from(msg)),
                headers: vec![],
                header_map: None,
                preserve_keepalive: false,
            }))
        }
    }
}
