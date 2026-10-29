#![allow(
    clippy::missing_docs_in_private_items,
    clippy::doc_markdown,
    reason = "internal filter implementation; public items are documented"
)]
// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 Praxis Contributors

//! Per-replica QoS admission guard.
//!
//! This filter is intentionally local and approximate. The authoritative class
//! comes from metering-service; this guard exists to keep a retry loop or a very
//! hot client from consuming every gateway replica while the class refresh is in
//! flight. It never authenticates or identifies callers; `external_metering`
//! publishes the verified class and canonical subject.

mod config;

#[cfg(test)]
#[expect(clippy::allow_attributes, reason = "blanket test suppressions")]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used, reason = "tests")]
mod tests;

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use bytes::Bytes;
use dashmap::DashMap;
use metrics::counter;
use praxis_filter::{FilterAction, FilterError, HttpFilter, HttpFilterContext, Rejection, parse_filter_config};
use tracing::debug;

use self::config::{BucketConfig, EnforcementMode, QosEnforcerConfig, validate_config};

/// Rejection status for an exhausted local bucket.
const STATUS_RATE_LIMITED: u16 = 429;

/// Idle lifetime after which a per-user bucket is reaped.
const IDLE_BUCKET_TTL: Duration = Duration::from_secs(10 * 60);

/// How often (in admissions) the idle sweep runs.
const SWEEP_EVERY: u64 = 1024;

/// The QoS admission enforcer.
pub struct QosEnforcerFilter {
    enabled: bool,
    clusters: Vec<String>,
    class_metadata: String,
    subject_metadata: String,
    enforcement: EnforcementMode,
    main: BucketLimits,
    premium: BucketLimits,
    max_keys: usize,
    buckets: DashMap<String, BucketState>,
    admissions: AtomicU64,
    rejection_body: Bytes,
}

#[derive(Clone, Copy, Debug)]
struct BucketLimits {
    burst: f64,
    refill_per_second: f64,
}

impl BucketLimits {
    fn from_config(cfg: BucketConfig) -> Self {
        Self {
            burst: f64::from(cfg.burst),
            refill_per_second: cfg.refill_per_second,
        }
    }

    fn disabled(self) -> bool {
        self.burst <= 0.0
    }
}

#[derive(Debug)]
struct BucketState {
    tokens: f64,
    last_refill: Instant,
    last_seen: Instant,
}

impl QosEnforcerFilter {
    /// Build from YAML config.
    ///
    /// # Errors
    ///
    /// Returns [`FilterError`] if config parsing or validation fails.
    pub fn from_config(value: &serde_yaml::Value) -> Result<Box<dyn HttpFilter>, FilterError> {
        let cfg: QosEnforcerConfig = parse_filter_config("qos_enforcer", value)?;
        validate_config(&cfg).map_err(FilterError::from)?;
        let rejection_body = serde_json::to_vec(&serde_json::json!({
            "error": {
                "message": "request rate exceeded for this QoS class",
                "type": "rate_limit_error",
                "code": "qos_rate_limited"
            }
        }))
        .map_err(|error| -> FilterError {
            format!("qos_enforcer: failed to serialize rejection body: {error}").into()
        })?;

        Ok(Box::new(Self {
            enabled: cfg.enabled,
            clusters: cfg.clusters,
            class_metadata: cfg.class_metadata,
            subject_metadata: cfg.subject_metadata,
            enforcement: cfg.enforcement,
            main: BucketLimits::from_config(cfg.main),
            premium: BucketLimits::from_config(cfg.premium),
            max_keys: cfg.max_keys,
            buckets: DashMap::new(),
            admissions: AtomicU64::new(0),
            rejection_body: Bytes::from(rejection_body),
        }))
    }

    fn applies_to_cluster(&self, ctx: &HttpFilterContext<'_>) -> bool {
        ctx.cluster
            .as_deref()
            .is_some_and(|cluster| self.clusters.iter().any(|candidate| candidate == cluster))
    }

    fn limits_for(&self, class: &str) -> BucketLimits {
        match class {
            "premium" => self.premium,
            _ => self.main,
        }
    }

    fn reject(&self, class: &str, retry_after: u64) -> FilterAction {
        FilterAction::Reject(
            Rejection::status(STATUS_RATE_LIMITED)
                .with_header("Retry-After", retry_after.to_string())
                .with_header("X-EnMaaS-QoS-Class", class.to_owned())
                .with_header("Content-Type", "application/json")
                .with_body(self.rejection_body.clone()),
        )
    }

    fn maybe_sweep(&self) {
        let n = self.admissions.fetch_add(1, Ordering::Relaxed);
        if n.is_multiple_of(SWEEP_EVERY) {
            let now = Instant::now();
            self.buckets
                .retain(|_, bucket| now.duration_since(bucket.last_seen) < IDLE_BUCKET_TTL);
        }
    }
}

#[async_trait]
#[expect(clippy::too_many_lines, reason = "the class branches share one admission loop")]
impl HttpFilter for QosEnforcerFilter {
    fn name(&self) -> &'static str {
        "qos_enforcer"
    }

    async fn on_request(&self, ctx: &mut HttpFilterContext<'_>) -> Result<FilterAction, FilterError> {
        if !self.enabled || !self.applies_to_cluster(ctx) {
            return Ok(FilterAction::Continue);
        }

        let class = ctx
            .get_metadata(&self.class_metadata)
            .filter(|value| matches!(*value, "premium" | "main" | "ratelimit"))
            .unwrap_or("main");
        let subject = ctx.get_metadata(&self.subject_metadata).unwrap_or("unknown");

        if class == "ratelimit" {
            match self.enforcement {
                EnforcementMode::Hard => {
                    counter!(
                        "praxis_ai_qos_requests_total",
                        "class" => "ratelimit".to_owned(),
                        "result" => "denied".to_owned(),
                    )
                    .increment(1);
                    debug!("qos_enforcer: ratelimit class rejected");
                    let retry = ctx
                        .get_metadata("qos.retry_after")
                        .and_then(|value| value.parse::<u64>().ok())
                        .unwrap_or(30)
                        .max(1);
                    return Ok(self.reject(class, retry));
                },
                EnforcementMode::Soft | EnforcementMode::Off => {
                    counter!(
                        "praxis_ai_qos_requests_total",
                        "class" => "ratelimit".to_owned(),
                        "result" => "soft".to_owned(),
                    )
                    .increment(1);
                    return Ok(FilterAction::Continue);
                },
            }
        }

        if self.enforcement != EnforcementMode::Hard {
            counter!(
                "praxis_ai_qos_requests_total",
                "class" => class.to_owned(),
                "result" => "observed".to_owned(),
            )
            .increment(1);
            return Ok(FilterAction::Continue);
        }

        let limits = self.limits_for(class);
        if limits.disabled() {
            return Ok(FilterAction::Continue);
        }

        self.maybe_sweep();
        let key = format!("{class}\u{1f}{subject}");
        let now = Instant::now();
        if !self.buckets.contains_key(&key) && self.buckets.len() >= self.max_keys {
            counter!(
                "praxis_ai_qos_requests_total",
                "class" => class.to_owned(),
                "result" => "key_capacity".to_owned(),
            )
            .increment(1);
            return Ok(self.reject(class, 30));
        }

        let (allowed, retry_after) = {
            let mut bucket = self.buckets.entry(key).or_insert_with(|| BucketState {
                tokens: limits.burst,
                last_refill: now,
                last_seen: now,
            });
            let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
            bucket.tokens = (bucket.tokens + elapsed * limits.refill_per_second).min(limits.burst);
            bucket.last_refill = now;
            bucket.last_seen = now;
            if bucket.tokens >= 1.0 {
                bucket.tokens -= 1.0;
                drop(bucket);
                (true, 0)
            } else {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "retry seconds are positive and bounded by the bucket refill"
                )]
                let retry = ((1.0 - bucket.tokens) / limits.refill_per_second).ceil() as u64;
                drop(bucket);
                (false, retry.max(1))
            }
        };

        let result = if allowed { "admitted" } else { "denied" };
        counter!(
            "praxis_ai_qos_requests_total",
            "class" => class.to_owned(),
            "result" => result.to_owned(),
        )
        .increment(1);

        if allowed {
            Ok(FilterAction::Continue)
        } else {
            debug!(class, retry_after, "qos_enforcer: local bucket exhausted");
            Ok(self.reject(class, retry_after))
        }
    }
}
