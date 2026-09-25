// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Praxis Contributors

//! Rewrites `reasoning_effort` values for backends with a restricted
//! effort vocabulary.
//!
//! The Qwen3.8 chat template (vLLM) only accepts `xhigh`, `medium`, and
//! `low` for `reasoning_effort`. `high` — the default most OpenAI-style
//! clients send — and `minimal` are rejected with a 400 `BadRequestError`.
//! The Responses-to-chat bridge copies `reasoning.effort` into
//! `reasoning_effort` verbatim, so Codex-style clients on
//! `/v1/responses` hit the same wall as plain chat clients.
//!
//! This filter rewrites effort values in the request body for both wire
//! shapes — the chat-completions top-level `reasoning_effort` string and
//! the Responses `reasoning.effort` field — using a configurable map
//! (default: `high -> xhigh`, `minimal -> low`).
//!
//! Rewriting only applies to requests whose body `model` field names one
//! of the configured `models`, so providers with a wider vocabulary (e.g.
//! `gpt-*` models that legitimately accept `high`) pass through
//! untouched. The `models` list is mandatory and must be non-empty: the
//! filter refuses to start instead of silently rewriting every backend.
//!
//! Gating is on the body's `model` field, not on the router's selected
//! cluster, because that is the only scope a body-mutating filter can
//! observe: when the chain buffers the request body (e.g. a `model_to_header`
//! filter precedes the router), the protocol pre-reads the body and runs
//! every filter's body hook in that pass, before any filter's header
//! phase — so the cluster selection does not exist yet at mutation time.
//! With catalog models routing one-to-one to clusters, a per-model list
//! expresses exactly the cluster scoping, and a missing entry fails
//! loudly (the backend 400s) instead of silently mis-rewriting.
//!
//! # YAML
//!
//! ```yaml
//! filter: reasoning_effort_map
//! models: ["Inferact/Qwen3.8-Flash-Next-NVFP4"]   # only these bodies are rewritten
//! values:                    # defaults shown
//!   high: "xhigh"
//!   minimal: "low"
//! ```
//!
//! Place this filter **after** `content_normalize` (so the rewrite sees
//! the normalized body) and **before** the `load_balancer` in any chain
//! that can route to a backend with a restricted effort vocabulary.

#[cfg(test)]
#[expect(clippy::allow_attributes, reason = "blanket test suppressions")]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "tests"
)]
mod tests;

use std::collections::BTreeMap;

use async_trait::async_trait;
use bytes::Bytes;
use praxis_ai_apis::json_body::replace_json_body;
use praxis_filter::{
    BodyAccess, BodyMode, FilterAction, FilterError, HttpFilter, HttpFilterContext, parse_filter_config,
};
use serde_json::Value;
use tracing::debug;

/// Default maximum request body bytes.
const DEFAULT_MAX_BODY_BYTES: usize = 4_194_304; // 4 MiB — agentic sessions send large contexts

/// Default for `max_body_bytes`.
fn default_max_body_bytes() -> usize {
    DEFAULT_MAX_BODY_BYTES
}

/// Default effort rewrites: values the Qwen3.8 chat template rejects,
/// mapped to its nearest accepted neighbor.
fn default_effort_values() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("high".to_owned(), "xhigh".to_owned()),
        ("minimal".to_owned(), "low".to_owned()),
    ])
}

/// Parsed YAML config for the `reasoning_effort_map` filter.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ReasoningEffortMapConfig {
    /// Requested model names whose bodies this filter rewrites (exact
    /// match on the body `model` field). Bodies naming any other model
    /// pass through unchanged. Required and must be non-empty.
    models: Vec<String>,
    /// Requested effort value -> replacement value. Defaults to
    /// `high -> xhigh`, `minimal -> low`.
    #[serde(default = "default_effort_values")]
    values: BTreeMap<String, String>,
    /// Maximum request body size accepted by the filter.
    #[serde(default = "default_max_body_bytes")]
    max_body_bytes: usize,
}

/// Rewrites `reasoning_effort` / `reasoning.effort` for configured
/// models so clients can send effort values the backend rejects.
pub struct ReasoningEffortMapFilter {
    /// Model names whose request bodies are rewritten.
    models: Vec<String>,
    /// Effort value rewrites applied to matching requests.
    values: BTreeMap<String, String>,
    /// Maximum request body size accepted by the filter.
    max_body_bytes: usize,
}

impl ReasoningEffortMapFilter {
    /// Create from parsed YAML config.
    ///
    /// # Errors
    ///
    /// Returns [`FilterError`] if the YAML config is invalid or if
    /// `models` is missing/empty — an unscoped rewrite would silently
    /// change effort for backends that accept it.
    pub fn from_config(config: &serde_yaml::Value) -> Result<Box<dyn HttpFilter>, FilterError> {
        let cfg: ReasoningEffortMapConfig = parse_filter_config("reasoning_effort_map", config)?;
        if cfg.models.is_empty() {
            return Err("reasoning_effort_map: `models` must list at least one model".into());
        }
        Ok(Box::new(Self {
            models: cfg.models,
            values: cfg.values,
            max_body_bytes: cfg.max_body_bytes,
        }))
    }
}

/// Rewrites `container[key]` through `values` when it is a mapped
/// string. Returns `true` if the value changed.
fn remap_field(container: &mut Value, key: &str, values: &BTreeMap<String, String>) -> bool {
    let Some(current) = container.get(key).and_then(Value::as_str) else {
        return false;
    };
    let Some(replacement) = values.get(current) else {
        return false;
    };
    if replacement == current {
        return false;
    }
    container[key] = Value::String(replacement.clone());
    true
}

#[async_trait]
impl HttpFilter for ReasoningEffortMapFilter {
    fn name(&self) -> &'static str {
        "reasoning_effort_map"
    }

    fn request_body_access(&self) -> BodyAccess {
        BodyAccess::ReadWrite
    }

    fn request_body_mode(&self) -> BodyMode {
        BodyMode::StreamBuffer {
            max_bytes: Some(self.max_body_bytes),
        }
    }

    async fn on_request(&self, _ctx: &mut HttpFilterContext<'_>) -> Result<FilterAction, FilterError> {
        Ok(FilterAction::Continue)
    }

    async fn on_request_body(
        &self,
        _ctx: &mut HttpFilterContext<'_>,
        body: &mut Option<Bytes>,
        end_of_stream: bool,
    ) -> Result<FilterAction, FilterError> {
        if !end_of_stream {
            return Ok(FilterAction::Continue);
        }

        let Some(raw) = body.as_ref() else {
            return Ok(FilterAction::Continue);
        };

        let mut value: Value = match serde_json::from_slice(raw) {
            Ok(v) => v,
            Err(_) => return Ok(FilterAction::Continue),
        };

        // Gate on the body's own model — the only routing scope visible
        // from a body hook (see module docs).
        if !value
            .get("model")
            .and_then(Value::as_str)
            .is_some_and(|m| self.models.iter().any(|s| s == m))
        {
            return Ok(FilterAction::Continue);
        }

        // Chat-completions shape: top-level reasoning_effort string.
        let mut mutated = remap_field(&mut value, "reasoning_effort", &self.values);
        // Responses shape: reasoning.effort (only when reasoning is an
        // object; anything else is left for the backend to judge).
        if let Some(reasoning) = value.get_mut("reasoning") {
            mutated |= remap_field(reasoning, "effort", &self.values);
        }

        if mutated {
            debug!("rewrote reasoning effort for a backend with a restricted vocabulary");
            replace_json_body(body, &value, "reasoning_effort_map", "reasoning effort")
                .map_err(|e| -> FilterError { format!("reasoning_effort_map: {e}").into() })?;
        }

        Ok(FilterAction::Continue)
    }
}
