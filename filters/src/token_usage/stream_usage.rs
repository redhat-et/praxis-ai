// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Praxis Contributors

//! Injects `stream_options.include_usage` into OpenAI streaming requests.
//!
//! OpenAI Chat Completions only reports token usage in a stream when the
//! client sends `stream_options: {"include_usage": true}`. Without it the
//! response carries no usage object and the gateway meters a legitimate 0.
//!
//! This filter ensures every streaming chat-completions request carries the
//! opt-in, so metering works regardless of what the client sends.
//!
//! Non-streaming requests and bodies that already carry the opt-in pass
//! through untouched.
//!
//! # YAML
//!
//! ```yaml
//! filter: stream_usage_inject
//! ```

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

use async_trait::async_trait;
use bytes::Bytes;
use praxis_ai_apis::json_body::replace_json_body;
use praxis_filter::{
    BodyAccess, BodyMode, FilterAction, FilterError, HttpFilter, HttpFilterContext,
    parse_filter_config,
};
use serde::Deserialize;
use serde_json::Value;
use tracing::debug;

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

/// Only Chat Completions supports `stream_options`; Responses API does not.
fn is_chat_completions(ctx: &HttpFilterContext<'_>) -> bool {
    ctx.request.uri.path().ends_with("/chat/completions")
}

/// Returns `true` when the body is a streaming request without `include_usage`.
fn needs_injection(value: &Value) -> bool {
    value.get("stream") == Some(&Value::Bool(true))
        && value
            .get("stream_options")
            .and_then(|so| so.get("include_usage"))
            != Some(&Value::Bool(true))
}

/// Sets `stream_options.include_usage = true`, creating the object if needed.
fn inject_include_usage(value: &mut Value) -> Result<(), FilterError> {
    let obj = value
        .as_object_mut()
        .ok_or_else(|| FilterError::from("stream_usage_inject: body is not a JSON object"))?;

    obj.entry("stream_options")
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| FilterError::from("stream_usage_inject: stream_options is not an object"))?
        .insert("include_usage".to_owned(), Value::Bool(true));

    debug!("injected stream_options.include_usage=true");
    Ok(())
}

/// Default maximum request body bytes for `StreamBuffer` mode (1 MiB).
const DEFAULT_MAX_BODY_BYTES: usize = 1_048_576;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Deserialized YAML config for `stream_usage_inject`.
struct StreamUsageConfig {
    /// Maximum request body bytes for `StreamBuffer` mode.
    #[serde(default = "default_max_body_bytes")]
    max_body_bytes: usize,
}

/// Returns the default max body bytes.
fn default_max_body_bytes() -> usize {
    DEFAULT_MAX_BODY_BYTES
}

/// Injects `stream_options.include_usage = true` into streaming OpenAI
/// chat-completions requests so the upstream response contains token usage.
pub struct StreamUsageInjectFilter {
    /// Maximum request body bytes for `StreamBuffer` mode.
    max_body_bytes: usize,
}

impl StreamUsageInjectFilter {
    /// Create from parsed YAML config.
    ///
    /// # Errors
    ///
    /// Returns [`FilterError`] if the YAML config is invalid.
    pub fn from_config(config: &serde_yaml::Value) -> Result<Box<dyn HttpFilter>, FilterError> {
        let cfg: StreamUsageConfig = parse_filter_config("stream_usage_inject", config)?;
        Ok(Box::new(Self {
            max_body_bytes: cfg.max_body_bytes,
        }))
    }
}

#[async_trait]
impl HttpFilter for StreamUsageInjectFilter {
    fn name(&self) -> &'static str {
        "stream_usage_inject"
    }

    fn request_body_access(&self) -> BodyAccess {
        BodyAccess::ReadWrite
    }

    fn request_body_mode(&self) -> BodyMode {
        BodyMode::StreamBuffer {
            max_bytes: Some(self.max_body_bytes),
        }
    }

    async fn on_request(
        &self,
        _ctx: &mut HttpFilterContext<'_>,
    ) -> Result<FilterAction, FilterError> {
        Ok(FilterAction::Continue)
    }

    async fn on_request_body(
        &self,
        ctx: &mut HttpFilterContext<'_>,
        body: &mut Option<Bytes>,
        end_of_stream: bool,
    ) -> Result<FilterAction, FilterError> {
        if !end_of_stream {
            return Ok(FilterAction::Continue);
        }

        if !is_chat_completions(ctx) {
            return Ok(FilterAction::Continue);
        }

        let Some(raw) = body.as_ref() else {
            return Ok(FilterAction::Continue);
        };

        let mut value: Value = match serde_json::from_slice(raw) {
            Ok(v) => v,
            Err(_) => return Ok(FilterAction::Continue),
        };

        if needs_injection(&value) {
            inject_include_usage(&mut value)?;
            replace_json_body(body, &value, "stream_usage_inject", "stream_options")
                .map_err(|e| -> FilterError { format!("stream_usage_inject: {e}").into() })?;
        }

        Ok(FilterAction::Continue)
    }
}
