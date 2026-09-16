// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Praxis Contributors

//! Normalizes Anthropic content block types for vLLM compatibility.
//!
//! vLLM's Anthropic endpoint only accepts a fixed set of content block
//! types: `text`, `image`, `tool_use`, `tool_result`, `tool_reference`,
//! `thinking`, and `redacted_thinking`. Newer Anthropic API types like
//! `server_tool_use` and `server_tool_result` (used by Claude Code for
//! MCP and web search) are rejected with a 400 error.
//!
//! Additionally, `tool_reference` blocks inside `tool_result` content
//! pass vLLM validation but crash Qwen's chat template, which only
//! handles `text` and `image` in content arrays.
//!
//! This filter rewrites unsupported types to their supported equivalents
//! so requests pass through vLLM unchanged in semantics but with
//! compatible content block types.
//!
//! # YAML
//!
//! ```yaml
//! filter: content_normalize
//! ```
//!
//! Place this filter **before** the `load_balancer` in any chain that
//! routes to a vLLM backend speaking the Anthropic protocol.

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
use serde_json::Value;
use tracing::debug;

/// Content block types that vLLM accepts in the Anthropic endpoint.
const VLLM_ACCEPTED_TYPES: &[&str] = &[
    "text",
    "image",
    "tool_use",
    "tool_result",
    "thinking",
    "redacted_thinking",
];

/// Content block types that Qwen's chat template accepts inside
/// `tool_result` content arrays (a stricter subset).
const QWEN_TOOL_RESULT_TYPES: &[&str] = &["text", "image"];

/// Default maximum request body bytes.
const DEFAULT_MAX_BODY_BYTES: usize = 4_194_304; // 4 MiB — agentic sessions send large contexts

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ContentNormalizeConfig {
    #[serde(default = "default_max_body_bytes")]
    max_body_bytes: usize,
}

fn default_max_body_bytes() -> usize {
    DEFAULT_MAX_BODY_BYTES
}

/// Normalizes Anthropic content block types for vLLM/Qwen compatibility.
pub struct ContentNormalizeFilter {
    max_body_bytes: usize,
}

impl ContentNormalizeFilter {
    /// Create from parsed YAML config.
    ///
    /// # Errors
    ///
    /// Returns [`FilterError`] if the YAML config is invalid.
    pub fn from_config(config: &serde_yaml::Value) -> Result<Box<dyn HttpFilter>, FilterError> {
        let cfg: ContentNormalizeConfig = parse_filter_config("content_normalize", config)?;
        Ok(Box::new(Self {
            max_body_bytes: cfg.max_body_bytes,
        }))
    }
}

/// Walks all messages and normalizes unsupported content block types.
/// Returns `true` if any block was rewritten.
fn normalize_messages(messages: &mut Vec<Value>) -> bool {
    let mut mutated = false;
    for msg in messages {
        let Some(content) = msg.get_mut("content") else {
            continue;
        };
        let Some(blocks) = content.as_array_mut() else {
            continue;
        };
        for block in blocks.iter_mut() {
            let Some(block_obj) = block.as_object_mut() else {
                continue;
            };
            let Some(type_val) = block_obj.get("type").and_then(Value::as_str) else {
                continue;
            };

            match type_val {
                "server_tool_use" => {
                    block_obj.insert("type".to_owned(), Value::String("tool_use".to_owned()));
                    mutated = true;
                }
                "server_tool_result" => {
                    block_obj.insert("type".to_owned(), Value::String("tool_result".to_owned()));
                    mutated = true;
                }
                "tool_result" => {
                    if let Some(inner) = block_obj.get_mut("content") {
                        if normalize_tool_result_content(inner) {
                            mutated = true;
                        }
                    }
                }
                t if !VLLM_ACCEPTED_TYPES.contains(&t) => {
                    let original_type = t.to_owned();
                    let text = extract_text_from_block(block_obj, &original_type);
                    block_obj.clear();
                    block_obj.insert("type".to_owned(), Value::String("text".to_owned()));
                    block_obj.insert("text".to_owned(), Value::String(text));
                    mutated = true;
                }
                _ => {}
            }
        }
    }
    mutated
}

/// Normalizes content blocks inside a `tool_result` for Qwen's template.
/// Returns `true` if any block was rewritten.
fn normalize_tool_result_content(content: &mut Value) -> bool {
    let Some(blocks) = content.as_array_mut() else {
        return false;
    };
    let mut mutated = false;
    for block in blocks.iter_mut() {
        let Some(block_obj) = block.as_object_mut() else {
            continue;
        };
        let Some(type_val) = block_obj.get("type").and_then(Value::as_str) else {
            continue;
        };
        if !QWEN_TOOL_RESULT_TYPES.contains(&type_val) {
            let original_type = type_val.to_owned();
            let text = extract_text_from_block(block_obj, &original_type);
            block_obj.clear();
            block_obj.insert("type".to_owned(), Value::String("text".to_owned()));
            block_obj.insert("text".to_owned(), Value::String(text));
            mutated = true;
        }
    }
    mutated
}

/// Best-effort text extraction from a content block being replaced.
fn extract_text_from_block(block: &serde_json::Map<String, Value>, original_type: &str) -> String {
    if let Some(text) = block.get("text").and_then(Value::as_str) {
        return text.to_owned();
    }
    if let Some(name) = block.get("name").and_then(Value::as_str) {
        return format!("[{original_type}: {name}]");
    }
    if let Some(name) = block.get("tool_name").and_then(Value::as_str) {
        return format!("[{original_type}: {name}]");
    }
    format!("[{original_type}]")
}

#[async_trait]
impl HttpFilter for ContentNormalizeFilter {
    fn name(&self) -> &'static str {
        "content_normalize"
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

        let Some(messages) = value.get_mut("messages").and_then(Value::as_array_mut) else {
            return Ok(FilterAction::Continue);
        };

        if normalize_messages(messages) {
            debug!("normalized unsupported content block types for vLLM compatibility");
            replace_json_body(body, &value, "content_normalize", "messages")
                .map_err(|e| -> FilterError { format!("content_normalize: {e}").into() })?;
        }

        Ok(FilterAction::Continue)
    }
}
