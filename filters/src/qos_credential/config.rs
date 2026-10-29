#![allow(
    clippy::missing_docs_in_private_items,
    clippy::doc_markdown,
    reason = "internal filter implementation; public items are documented"
)]
// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 Praxis Contributors

//! Configuration for the QoS credential selector.

use std::collections::BTreeMap;

use serde::Deserialize;

const DEFAULT_CLUSTER: &str = "cb-glm";
const DEFAULT_HEADER: &str = "authorization";
const DEFAULT_HEADER_PREFIX: &str = "Bearer ";
const DEFAULT_CLASS_METADATA: &str = "qos.class";
const DEFAULT_SUBJECT_METADATA: &str = "qos.subject";
const DEFAULT_CLASS: &str = "main";
const DEFAULT_KEY_ENV: &str = "CB_LITELLM_API_KEY";

/// Deserialized YAML config for `qos_credential`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct QosCredentialConfig {
    /// Master switch. Disabled injects the default provider key and leaves
    /// class selection off.
    #[serde(default)]
    pub enabled: bool,

    /// Observe the selected class key but inject the main/default key.
    #[serde(default)]
    pub shadow: bool,

    /// Only inject credentials for these routed clusters.
    #[serde(default = "default_clusters")]
    pub clusters: Vec<String>,

    /// Header carrying the provider credential.
    #[serde(default = "default_header")]
    pub header: String,

    /// Prefix prepended to the key value.
    #[serde(default = "default_header_prefix")]
    pub header_prefix: String,

    /// Metadata key carrying the verified class.
    #[serde(default = "default_class_metadata")]
    pub class_metadata: String,

    /// Metadata key carrying the canonical per-person subject.
    #[serde(default = "default_subject_metadata")]
    pub subject_metadata: String,

    /// Class used when no class metadata is present.
    #[serde(default = "default_class")]
    pub default_class: String,

    /// Optional class-to-key-class aliases. Defaults to ratelimit -> main
    /// for the two-key Phase 1 deployment.
    #[serde(default = "default_aliases")]
    pub class_aliases: BTreeMap<String, String>,

    /// Secret-mounted JSON key map. Required when enabled.
    #[serde(default)]
    pub key_file: Option<String>,

    /// Environment variable holding the legacy/default provider key. Used
    /// when class selection is disabled or a class has no key.
    #[serde(default = "default_key_env")]
    pub default_key_env: String,
}

/// One key ring entry.
#[derive(Clone, Deserialize)]
pub(super) struct KeyEntry {
    /// Stable, non-secret identifier used in logs and metrics.
    pub id: String,
    /// Provider API key value.
    pub key: String,
}

impl std::fmt::Debug for KeyEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyEntry")
            .field("id", &self.id)
            .field("key", &"[REDACTED]")
            .finish()
    }
}

/// JSON key map mounted from a Secret.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct KeyFile {
    #[serde(default)]
    pub premium: Vec<KeyEntry>,
    #[serde(default)]
    pub main: Vec<KeyEntry>,
    #[serde(default)]
    pub ratelimit: Vec<KeyEntry>,
}

fn default_clusters() -> Vec<String> {
    vec![DEFAULT_CLUSTER.to_owned()]
}

fn default_header() -> String {
    DEFAULT_HEADER.to_owned()
}

fn default_header_prefix() -> String {
    DEFAULT_HEADER_PREFIX.to_owned()
}

fn default_class_metadata() -> String {
    DEFAULT_CLASS_METADATA.to_owned()
}

fn default_subject_metadata() -> String {
    DEFAULT_SUBJECT_METADATA.to_owned()
}

fn default_class() -> String {
    DEFAULT_CLASS.to_owned()
}

fn default_aliases() -> BTreeMap<String, String> {
    BTreeMap::from([("ratelimit".to_owned(), "main".to_owned())])
}

fn default_key_env() -> String {
    DEFAULT_KEY_ENV.to_owned()
}

/// Validate a config and key file.
#[expect(
    clippy::too_many_lines,
    reason = "all credential-ring invariants are validated in one place"
)]
pub(super) fn validate_config(cfg: &QosCredentialConfig, keys: &KeyFile) -> Result<(), String> {
    if cfg.clusters.is_empty() || cfg.clusters.iter().any(|c| c.trim().is_empty()) {
        return Err("qos_credential: clusters must contain at least one non-empty name".into());
    }
    if cfg.class_metadata.is_empty() || cfg.subject_metadata.is_empty() {
        return Err("qos_credential: class_metadata and subject_metadata must not be empty".into());
    }
    if http::header::HeaderName::from_bytes(cfg.header.as_bytes()).is_err() {
        return Err("qos_credential: header must be a valid HTTP header name".into());
    }
    if cfg.header_prefix.chars().any(char::is_control) {
        return Err("qos_credential: header_prefix must not contain control characters".into());
    }
    for class in [cfg.default_class.as_str()]
        .into_iter()
        .chain(cfg.class_aliases.keys().map(String::as_str))
        .chain(cfg.class_aliases.values().map(String::as_str))
    {
        if !matches!(class, "premium" | "main" | "ratelimit") {
            return Err(format!("qos_credential: invalid class '{class}'"));
        }
    }
    for (class, entries) in [
        ("premium", &keys.premium),
        ("main", &keys.main),
        ("ratelimit", &keys.ratelimit),
    ] {
        let mut ids = std::collections::HashSet::new();
        for entry in entries {
            if entry.id.trim().is_empty() {
                return Err(format!("qos_credential: {class} key id must not be empty"));
            }
            if !ids.insert(entry.id.clone()) {
                return Err(format!("qos_credential: duplicate {class} key id '{}'", entry.id));
            }
            if entry.key.is_empty() || entry.key.chars().any(char::is_control) {
                return Err(format!("qos_credential: {class} key '{}' is invalid", entry.id));
            }
            let value = format!("{}{}", cfg.header_prefix, entry.key);
            if http::HeaderValue::from_str(&value).is_err() {
                return Err(format!(
                    "qos_credential: {class} key '{}' produces an invalid header",
                    entry.id
                ));
            }
        }
    }
    if cfg.enabled && cfg.key_file.as_deref().is_none_or(str::is_empty) {
        return Err("qos_credential: key_file is required when enabled".into());
    }
    Ok(())
}
