#![allow(
    clippy::missing_docs_in_private_items,
    clippy::doc_markdown,
    reason = "internal filter implementation; public items are documented"
)]
// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 Praxis Contributors

//! Configuration for the QoS admission enforcer.

use serde::Deserialize;

const DEFAULT_CLASS_METADATA: &str = "qos.class";
const DEFAULT_SUBJECT_METADATA: &str = "qos.subject";
const DEFAULT_MAX_KEYS: usize = 100_000;
const DEFAULT_CLUSTER: &str = "cb-glm";

/// What the filter does when a class exceeds its local bucket.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(super) enum EnforcementMode {
    /// Disabled: observe nothing and change no request.
    #[default]
    Off,
    /// Count decisions and emit metrics, but do not reject.
    Soft,
    /// Enforce the per-class buckets and reject over-budget requests.
    Hard,
}

/// A request token bucket for one class.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BucketConfig {
    /// Maximum number of requests held at once. Zero disables the bucket.
    #[serde(default)]
    pub burst: u32,
    /// Requests replenished per second.
    #[serde(default)]
    pub refill_per_second: f64,
}

impl Default for BucketConfig {
    fn default() -> Self {
        Self {
            burst: 0,
            refill_per_second: 0.0,
        }
    }
}

/// Deserialized YAML config for `qos_enforcer`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct QosEnforcerConfig {
    /// Master switch. When false the filter is a no-op.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Only enforce on these routed clusters.
    #[serde(default = "default_clusters")]
    pub clusters: Vec<String>,

    /// Metadata key carrying the verified class.
    #[serde(default = "default_class_metadata")]
    pub class_metadata: String,

    /// Metadata key carrying the canonical per-person subject.
    #[serde(default = "default_subject_metadata")]
    pub subject_metadata: String,

    /// Enforcement mode.
    #[serde(default)]
    pub enforcement: EnforcementMode,

    /// Bucket for the `main` class.
    #[serde(default = "default_main_bucket")]
    pub main: BucketConfig,

    /// Optional safety bucket for the `premium` class. Zero disables it.
    #[serde(default)]
    pub premium: BucketConfig,

    /// Soft cap on the number of per-user buckets retained.
    #[serde(default = "default_max_keys")]
    pub max_keys: usize,
}

fn default_true() -> bool {
    true
}

fn default_clusters() -> Vec<String> {
    vec![DEFAULT_CLUSTER.to_owned()]
}

fn default_class_metadata() -> String {
    DEFAULT_CLASS_METADATA.to_owned()
}

fn default_subject_metadata() -> String {
    DEFAULT_SUBJECT_METADATA.to_owned()
}

fn default_main_bucket() -> BucketConfig {
    BucketConfig {
        burst: 10,
        refill_per_second: 1.0,
    }
}

fn default_max_keys() -> usize {
    DEFAULT_MAX_KEYS
}

/// Validate the filter configuration.
pub(super) fn validate_config(cfg: &QosEnforcerConfig) -> Result<(), String> {
    if cfg.class_metadata.is_empty() || cfg.subject_metadata.is_empty() {
        return Err("qos_enforcer: class_metadata and subject_metadata must not be empty".into());
    }
    if cfg.clusters.is_empty() || cfg.clusters.iter().any(|c| c.trim().is_empty()) {
        return Err("qos_enforcer: clusters must contain at least one non-empty name".into());
    }
    if cfg.max_keys == 0 {
        return Err("qos_enforcer: max_keys must be greater than 0".into());
    }
    validate_bucket("main", cfg.main)?;
    validate_bucket("premium", cfg.premium)?;
    Ok(())
}

fn validate_bucket(name: &str, bucket: BucketConfig) -> Result<(), String> {
    if bucket.burst > 0 && (!bucket.refill_per_second.is_finite() || bucket.refill_per_second <= 0.0) {
        return Err(format!(
            "qos_enforcer: {name}.refill_per_second must be positive when burst is set"
        ));
    }
    if bucket.burst == 0 && bucket.refill_per_second != 0.0 {
        return Err(format!(
            "qos_enforcer: {name}.refill_per_second must be zero when burst is disabled"
        ));
    }
    Ok(())
}
