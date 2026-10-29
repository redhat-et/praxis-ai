#![allow(
    clippy::missing_docs_in_private_items,
    clippy::doc_markdown,
    reason = "internal filter implementation; public items are documented"
)]
// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 Praxis Contributors

//! Class-aware Curvebender credential selection.
//!
//! EnMaaS does not send QoS headers to LiteLLM. Instead, metering-service
//! publishes the verified class and canonical subject, and this filter selects
//! the provider API key whose LiteLLM metadata maps to the intended
//! `InferenceObjective` and fairness ID. The client request is untouched.
//!
//! Phase 1 rings contain one key per class. The rendezvous-hash code is
//! intentionally ring-size agnostic so adding shard keys later is a config and
//! Secret change, not a filter rewrite.

mod config;

#[cfg(test)]
#[expect(clippy::allow_attributes, reason = "blanket test suppressions")]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used, reason = "tests")]
mod tests;

use std::{borrow::Cow, collections::HashMap, env, fs};

use async_trait::async_trait;
use bytes::Bytes;
use http::{HeaderName, HeaderValue};
use metrics::counter;
use praxis_ai_apis::hash::Sha256;
use praxis_filter::{FilterAction, FilterError, HttpFilter, HttpFilterContext, Rejection, parse_filter_config};
use tracing::debug;

use self::config::{KeyFile, QosCredentialConfig, validate_config};

/// Status used when no usable provider credential is available.
const STATUS_CREDENTIAL_UNAVAILABLE: u16 = 503;

/// A key ring entry with its prebuilt header value.
#[derive(Clone, Debug)]
struct ResolvedKey {
    id: String,
    header_value: String,
}

/// Class-aware provider credential selector.
pub struct QosCredentialFilter {
    enabled: bool,
    shadow: bool,
    clusters: Vec<String>,
    header: HeaderName,
    class_metadata: String,
    subject_metadata: String,
    default_class: String,
    class_aliases: HashMap<String, String>,
    keys: HashMap<String, Vec<ResolvedKey>>,
    default_key: Option<String>,
}

impl QosCredentialFilter {
    /// Build from YAML config.
    ///
    /// # Errors
    ///
    /// Returns [`FilterError`] if config parsing, key-file reading, or
    /// validation fails.
    #[expect(
        clippy::too_many_lines,
        reason = "config parsing, key-file reading, and ring construction stay together"
    )]
    pub fn from_config(value: &serde_yaml::Value) -> Result<Box<dyn HttpFilter>, FilterError> {
        let cfg: QosCredentialConfig = parse_filter_config("qos_credential", value)?;
        let key_file = if cfg.enabled {
            let path = cfg.key_file.as_deref().unwrap_or_default();
            let raw = fs::read_to_string(path).map_err(|error| -> FilterError {
                format!("qos_credential: key_file could not be read: {error}").into()
            })?;
            serde_json::from_str::<KeyFile>(&raw).map_err(|error| -> FilterError {
                format!("qos_credential: key_file is invalid JSON: {error}").into()
            })?
        } else {
            KeyFile::default()
        };
        validate_config(&cfg, &key_file).map_err(FilterError::from)?;

        let header = HeaderName::from_bytes(cfg.header.as_bytes())
            .map_err(|error| -> FilterError { format!("qos_credential: invalid header name: {error}").into() })?;
        let mut keys = HashMap::new();
        for (class, entries) in [
            ("premium", key_file.premium),
            ("main", key_file.main),
            ("ratelimit", key_file.ratelimit),
        ] {
            keys.insert(
                class.to_owned(),
                entries
                    .into_iter()
                    .map(|entry| ResolvedKey {
                        id: entry.id,
                        header_value: format!("{}{}", cfg.header_prefix, entry.key),
                    })
                    .collect(),
            );
        }
        let default_key = env::var(&cfg.default_key_env)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(|value| format!("{}{}", cfg.header_prefix, value.trim()))
            .filter(|value| HeaderValue::from_str(value).is_ok());

        Ok(Box::new(Self {
            enabled: cfg.enabled,
            shadow: cfg.shadow,
            clusters: cfg.clusters,
            header,
            class_metadata: cfg.class_metadata,
            subject_metadata: cfg.subject_metadata,
            default_class: cfg.default_class,
            class_aliases: cfg.class_aliases.into_iter().collect(),
            keys,
            default_key,
        }))
    }

    fn applies_to_cluster(&self, ctx: &HttpFilterContext<'_>) -> bool {
        ctx.cluster
            .as_deref()
            .is_some_and(|cluster| self.clusters.iter().any(|candidate| candidate == cluster))
    }

    fn normalize_class(&self, requested: &str) -> String {
        if matches!(requested, "premium" | "main" | "ratelimit") {
            requested.to_owned()
        } else {
            self.default_class.clone()
        }
    }

    fn entries_for(&self, class: &str) -> Option<&[ResolvedKey]> {
        self.keys
            .get(class)
            .filter(|entries| !entries.is_empty())
            .map(Vec::as_slice)
    }

    fn pick_key<'a>(subject: &str, entries: &'a [ResolvedKey]) -> Option<&'a ResolvedKey> {
        entries
            .iter()
            .map(|entry| {
                let mut digest = Sha256::new();
                digest.update(subject.as_bytes());
                digest.update(&[0x1f]);
                digest.update(entry.id.as_bytes());
                (digest.finish(), entry)
            })
            .max_by(|(left, _), (right, _)| left.cmp(right))
            .map(|(_, entry)| entry)
    }

    /// Resolve the key to inject for this request.
    ///
    /// Returns `None` when the filter is disabled or the selected class has no
    /// ring; the caller then uses the legacy default key.
    #[expect(
        clippy::too_many_lines,
        reason = "class aliasing and ring selection share one resolution path"
    )]
    fn resolve_key(&self, ctx: &HttpFilterContext<'_>) -> Option<ResolvedKey> {
        if !self.enabled {
            return None;
        }
        let requested = ctx
            .get_metadata(&self.class_metadata)
            .map_or_else(|| self.default_class.clone(), |value| self.normalize_class(value));
        let key_class = self
            .class_aliases
            .get(&requested)
            .map_or_else(|| requested.clone(), |value| self.normalize_class(value));
        let subject = ctx.get_metadata(&self.subject_metadata).unwrap_or("");
        let selected = Self::pick_key(subject, self.entries_for(&key_class).unwrap_or(&[]));

        let inject = if self.shadow {
            self.entries_for("main")
                .and_then(|entries| Self::pick_key(subject, entries))
                .or(selected)
        } else {
            selected
        };

        if let Some(key) = inject {
            debug!(
                requested_class = %requested,
                key_class = %key_class,
                key_id = %key.id,
                shadow = self.shadow,
                "qos_credential: selected provider key"
            );
            counter!(
                "praxis_ai_qos_key_selected_total",
                "requested_class" => requested,
                "key_id" => key.id.clone(),
                "shadow" => self.shadow.to_string(),
            )
            .increment(1);
            Some(key.clone())
        } else {
            // A class with no ring falls back to the legacy default key.
            None
        }
    }

    fn inject(ctx: &mut HttpFilterContext<'_>, header: &HeaderName, value: &str) -> Result<(), FilterAction> {
        if HeaderValue::from_str(value).is_err() {
            return Err(FilterAction::Reject(
                Rejection::status(STATUS_CREDENTIAL_UNAVAILABLE)
                    .with_body(Bytes::from_static(b"qos provider credential unavailable")),
            ));
        }
        ctx.extra_request_headers
            .push((Cow::Owned(header.as_str().to_owned()), value.to_owned()));
        Ok(())
    }
}

#[async_trait]
impl HttpFilter for QosCredentialFilter {
    fn name(&self) -> &'static str {
        "qos_credential"
    }

    async fn on_request(&self, ctx: &mut HttpFilterContext<'_>) -> Result<FilterAction, FilterError> {
        if !self.applies_to_cluster(ctx) {
            return Ok(FilterAction::Continue);
        }

        let selected = self.resolve_key(ctx);
        let value = selected
            .map(|key| key.header_value)
            .or_else(|| self.default_key.clone());
        let Some(value) = value else {
            return Ok(FilterAction::Reject(
                Rejection::status(STATUS_CREDENTIAL_UNAVAILABLE)
                    .with_body(Bytes::from_static(b"qos provider credential unavailable")),
            ));
        };

        match Self::inject(ctx, &self.header, &value) {
            Ok(()) => Ok(FilterAction::Continue),
            Err(action) => Ok(action),
        }
    }
}
