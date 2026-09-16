// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Praxis Contributors

//! Model catalog filter: answers `GET /v1/models` from static config
//! instead of forwarding it upstream.
//!
//! A multi-provider gateway fronts models from several backends (and
//! local models that have no upstream `/v1/models` at all), so no single
//! backend can enumerate what the gateway exposes. This filter serves
//! that list itself as a terminal response, in either the OpenAI or the
//! Anthropic envelope.
//!
//! This filter handles discovery only. Authorization — deciding who may
//! call a model — stays with `model_access`; the catalog does not
//! re-declare access rules.
//!
//! ```yaml
//! filter: model_catalog
//! format: openai            # openai | anthropic
//! path: /v1/models          # matched exactly on GET
//! models:
//!   - id: llama-3.1-8b-instruct
//!     owned_by: praxis
//!   - id: claude-internal
//!     display_name: "Claude (internal)"
//! ```

mod config;

#[cfg(test)]
#[expect(clippy::allow_attributes, reason = "blanket test suppressions")]
#[allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "tests"
)]
mod tests;

use async_trait::async_trait;
use praxis_filter::{
    BodyAccess, FilterAction, FilterError, HttpFilter, HttpFilterContext, TerminalResponse, parse_filter_config,
};
use serde_json::{Value, json};
use tracing::debug;

use self::config::{CatalogFormat, ModelCatalogConfig, ModelEntry, validate_config};

// -----------------------------------------------------------------------------
// CatalogModel
// -----------------------------------------------------------------------------

/// A compiled catalog entry ready to render.
struct CatalogModel {
    /// Model identifier returned to clients.
    id: String,
    /// Owner shown in the OpenAI envelope's `owned_by` field.
    owned_by: String,
    /// Human-readable name shown in the Anthropic envelope.
    display_name: String,
    /// Creation time as Unix seconds.
    created: i64,
}

impl CatalogModel {
    /// Compile a config entry, defaulting `display_name` to the id.
    fn compile(entry: ModelEntry) -> Self {
        let display_name = entry.display_name.unwrap_or_else(|| entry.id.clone());
        Self {
            id: entry.id,
            owned_by: entry.owned_by,
            display_name,
            created: entry.created,
        }
    }
}

// -----------------------------------------------------------------------------
// ModelCatalogFilter
// -----------------------------------------------------------------------------

/// Serves a configured model list as a terminal response to
/// `GET {path}`.
///
/// Discovery only: it advertises what the gateway exposes. Authorization
/// stays with `model_access`; this filter does not enforce access.
///
/// # YAML configuration
///
/// ```yaml
/// filter: model_catalog
/// format: anthropic
/// models:
///   - id: claude-internal
///     display_name: "Claude (internal)"
/// ```
///
/// # Example
///
/// ```rust
/// use praxis_ai_filters::ModelCatalogFilter;
///
/// let yaml: serde_yaml::Value = serde_yaml::from_str(
///     r#"
/// format: openai
/// models:
///   - id: llama-3.1-8b-instruct
/// "#,
/// )
/// .unwrap();
/// let filter = ModelCatalogFilter::from_config(&yaml).unwrap();
/// assert_eq!(filter.name(), "model_catalog");
/// ```
pub struct ModelCatalogFilter {
    /// Response envelope dialect.
    format: CatalogFormat,

    /// Path served, matched exactly on `GET`.
    path: String,

    /// Advertised models, in declaration order.
    models: Vec<CatalogModel>,
}

impl ModelCatalogFilter {
    /// Parse from YAML config.
    ///
    /// # Errors
    ///
    /// Returns [`FilterError`] if config parsing or validation fails.
    pub fn from_config(value: &serde_yaml::Value) -> Result<Box<dyn HttpFilter>, FilterError> {
        let cfg: ModelCatalogConfig = parse_filter_config("model_catalog", value)?;
        validate_config(&cfg).map_err(|e| -> FilterError { e.into() })?;

        let models = cfg.models.into_iter().map(CatalogModel::compile).collect();

        Ok(Box::new(Self {
            format: cfg.format,
            path: cfg.path,
            models,
        }))
    }

    /// Render the catalog body.
    fn render(&self) -> Vec<u8> {
        let value = match self.format {
            CatalogFormat::Openai => render_openai(&self.models),
            CatalogFormat::Anthropic => render_anthropic(&self.models),
        };
        serde_json::to_vec(&value).unwrap_or_else(|_| b"{}".to_vec())
    }
}

/// Build the OpenAI `{"object":"list","data":[...]}` envelope.
fn render_openai(models: &[CatalogModel]) -> Value {
    let data: Vec<Value> = models
        .iter()
        .map(|m| {
            json!({
                "id": m.id,
                "object": "model",
                "created": m.created,
                "owned_by": m.owned_by,
            })
        })
        .collect();
    json!({ "object": "list", "data": data })
}

/// Build the Anthropic `{"data":[...],"has_more":false,...}` envelope.
fn render_anthropic(models: &[CatalogModel]) -> Value {
    let data: Vec<Value> = models
        .iter()
        .map(|m| {
            json!({
                "type": "model",
                "id": m.id,
                "display_name": m.display_name,
                "created_at": rfc3339(m.created),
            })
        })
        .collect();
    json!({
        "data": data,
        "has_more": false,
        "first_id": models.first().map(|m| m.id.as_str()),
        "last_id": models.last().map(|m| m.id.as_str()),
    })
}

/// Format Unix seconds as an RFC 3339 UTC timestamp, defaulting to the
/// epoch for out-of-range values so rendering never fails.
fn rfc3339(secs: i64) -> String {
    chrono::DateTime::from_timestamp(secs, 0)
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[async_trait]
impl HttpFilter for ModelCatalogFilter {
    fn name(&self) -> &'static str {
        "model_catalog"
    }

    fn request_body_access(&self) -> BodyAccess {
        BodyAccess::None
    }

    async fn on_request(&self, ctx: &mut HttpFilterContext<'_>) -> Result<FilterAction, FilterError> {
        if ctx.request.method != http::Method::GET || ctx.request.uri.path() != self.path {
            return Ok(FilterAction::Continue);
        }

        let body = self.render();

        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_static("application/json"),
        );

        debug!(path = %self.path, "model_catalog: served model list");
        Ok(FilterAction::TerminalResponse(Box::new(
            TerminalResponse::new(200).with_headers(headers).with_body(body),
        )))
    }
}
