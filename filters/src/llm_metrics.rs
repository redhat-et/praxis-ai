// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 Praxis Contributors

//! LLM gateway metrics emitted by in-tree filters.
//!
//! These complement the proxy-owned `praxis_http_*` metrics (which live in
//! the external `praxis-proxy` crates) with model/provider-labeled signals
//! that the proxy cannot produce on its own: token accounting from backend
//! usage metadata, per-token decode timing, finish reasons, empty completions,
//! and precise body-size distributions.
//!
//! Every metric carries a `model` label for drill-down and a `cluster` label
//! for provider-level rollup, so both per-model and per-provider views are
//! one `sum by (...)` away.
//!
//! # Metric rationale
//!
//! - `praxis_ai_prompt_tokens_total` / `praxis_ai_output_tokens_total`:
//!   token volumes per model/provider (capacity, cost, output-length blowups).
//! - `praxis_ai_prompt_tokens` / `praxis_ai_output_tokens` (histograms):
//!   per-request token distributions (average prompt/output size per model).
//! - `praxis_ai_reasoning_tokens_total`: thinking-model behavior.
//! - `praxis_ai_cache_read_tokens_total`: prompt-cache hit volume (cost/latency).
//! - `praxis_ai_finish_reasons_total{reason}`: *why* generations end —
//!   `length` share = max-token/capacity friction, `content_filter` = policy.
//! - `praxis_ai_empty_completions_total`: zero-output responses = silent
//!   generation failures.
//! - `praxis_ai_tpot_seconds` (histogram): inter-token decode time
//!   (tokens/s ≈ 1/TPOT) — pure generation throughput.
//! - `praxis_ai_request_body_bytes` / `praxis_ai_response_body_bytes`
//!   (histograms): precise prompt/response size distributions. Bucket
//!   boundaries are exporter-configured; the desired 1MiB..32MiB ladder is
//!   set via the Prometheus exporter in the external `praxis-proxy` crates.

use std::sync::Once;

use metrics::{Unit, counter, describe_histogram, histogram};
use praxis_filter::HttpFilterContext;

/// Prompt tokens (backend-reported) per request, cumulative.
pub const METRIC_PROMPT_TOKENS_TOTAL: &str = "praxis_ai_prompt_tokens_total";
/// Output tokens (backend-reported) per request, cumulative.
pub const METRIC_OUTPUT_TOKENS_TOTAL: &str = "praxis_ai_output_tokens_total";
/// Prompt tokens per request, histogram.
pub const METRIC_PROMPT_TOKENS: &str = "praxis_ai_prompt_tokens";
/// Output tokens per request, histogram.
pub const METRIC_OUTPUT_TOKENS: &str = "praxis_ai_output_tokens";
/// Reasoning / thinking tokens (backend-reported), cumulative.
pub const METRIC_REASONING_TOKENS_TOTAL: &str = "praxis_ai_reasoning_tokens_total";
/// Input tokens served from the provider's prompt cache, cumulative.
pub const METRIC_CACHE_READ_TOKENS_TOTAL: &str = "praxis_ai_cache_read_tokens_total";
/// Responses that produced zero output tokens.
pub const METRIC_EMPTY_COMPLETIONS_TOTAL: &str = "praxis_ai_empty_completions_total";
/// Why a generation ended (`stop`, `length`, `content_filter`, `tool_calls`, ...).
pub const METRIC_FINISH_REASONS_TOTAL: &str = "praxis_ai_finish_reasons_total";
/// Time between consecutive streamed deltas (decode speed).
pub const METRIC_TPOT_SECONDS: &str = "praxis_ai_tpot_seconds";
/// Request body size per request, histogram.
pub const METRIC_REQUEST_BODY_BYTES: &str = "praxis_ai_request_body_bytes";
/// Response body size per request, histogram.
pub const METRIC_RESPONSE_BODY_BYTES: &str = "praxis_ai_response_body_bytes";

/// Body-size bucket ladder we want the exporter to apply: 1MiB through 32MiB
/// (2x steps), so the large-prompt/response tail has real resolution instead of
/// the proxy's coarse 1MiB->10MiB jump. metrics 0.24 has no per-metric
/// bucket API, so this ladder must be configured on the `metrics-exporter-prometheus`
/// builder in the external `praxis-proxy` crates.
#[expect(dead_code, reason = "referenced by the external prometheus exporter config")]
pub const BODY_BUCKETS: &[f64] = &[
    1.0 * 1024.0 * 1024.0,
    2.0 * 1024.0 * 1024.0,
    4.0 * 1024.0 * 1024.0,
    8.0 * 1024.0 * 1024.0,
    16.0 * 1024.0 * 1024.0,
    32.0 * 1024.0 * 1024.0,
];

/// Guards the one-time histogram descriptions.
static DESCRIBED: Once = Once::new();

/// Describe histogram units once so the Prometheus exporter renders sensible units.
fn ensure_described() {
    DESCRIBED.call_once(|| {
        describe_histogram!(METRIC_PROMPT_TOKENS, Unit::Count, "prompt tokens per request");
        describe_histogram!(METRIC_OUTPUT_TOKENS, Unit::Count, "output tokens per request");
        describe_histogram!(METRIC_TPOT_SECONDS, Unit::Seconds, "time per output token");
        describe_histogram!(METRIC_REQUEST_BODY_BYTES, Unit::Bytes, "request body bytes per request");
        describe_histogram!(METRIC_RESPONSE_BODY_BYTES, Unit::Bytes, "response body bytes per request");
    });
}

/// Resolve the `model` label from format/routing filter metadata, falling back
/// to `unknown` when no upstream format filter ran before this one.
pub fn resolve_model(ctx: &HttpFilterContext<'_>) -> String {
    ctx.get_metadata("provider_route.model")
        .or_else(|| ctx.get_metadata("openai_responses_format.model"))
        .or_else(|| ctx.get_metadata("anthropic_messages_format.model"))
        .or_else(|| ctx.get_metadata("anthropic_messages_to_chat_completions.model"))
        // The gateway config's model_to_header filter stamps the client-requested
        // model as an `X-Model` request header before token_count runs; fall back
        // to it so the label is populated even when no format filter ran.
        .or_else(|| ctx.request.headers.get("x-model").and_then(|v| v.to_str().ok()))
        .filter(|v| !v.is_empty())
        .unwrap_or("unknown")
        .to_owned()
}

/// Resolve the `cluster` (provider) label.
pub fn cluster(ctx: &HttpFilterContext<'_>) -> String {
    ctx.cluster
        .as_ref()
        .map_or_else(|| "unknown".to_owned(), ToString::to_string)
}

/// Emit per-request token, cache, reasoning, empty-completion and body-size
/// metrics from backend-reported usage.
#[expect(
    clippy::cast_precision_loss,
    reason = "token and byte counts are recorded as f64 histogram values"
)]
pub fn record_token_usage(
    ctx: &HttpFilterContext<'_>,
    input: u64,
    output: u64,
    cache_read: Option<u64>,
    reasoning: Option<u64>,
) {
    ensure_described();
    let model = resolve_model(ctx);
    let cluster = cluster(ctx);

    counter!(METRIC_PROMPT_TOKENS_TOTAL, "model" => model.clone(), "cluster" => cluster.clone()).increment(input);
    histogram!(METRIC_PROMPT_TOKENS, "model" => model.clone(), "cluster" => cluster.clone()).record(input as f64);

    counter!(METRIC_OUTPUT_TOKENS_TOTAL, "model" => model.clone(), "cluster" => cluster.clone()).increment(output);
    histogram!(METRIC_OUTPUT_TOKENS, "model" => model.clone(), "cluster" => cluster.clone()).record(output as f64);

    if let Some(cache_read) = cache_read {
        counter!(METRIC_CACHE_READ_TOKENS_TOTAL, "model" => model.clone(), "cluster" => cluster.clone())
            .increment(cache_read);
    }
    if let Some(reasoning) = reasoning {
        counter!(METRIC_REASONING_TOKENS_TOTAL, "model" => model.clone(), "cluster" => cluster.clone())
            .increment(reasoning);
    }
    if output == 0 {
        counter!(METRIC_EMPTY_COMPLETIONS_TOTAL, "model" => model.clone(), "cluster" => cluster.clone()).increment(1);
    }

    histogram!(METRIC_REQUEST_BODY_BYTES, "model" => model.clone(), "cluster" => cluster.clone())
        .record(ctx.request_body_bytes as f64);
    histogram!(METRIC_RESPONSE_BODY_BYTES, "model" => model.clone(), "cluster" => cluster.clone())
        .record(ctx.response_body_bytes as f64);
}

/// Emit an inter-delta (decode) timing sample.
pub fn record_tpot(ctx: &HttpFilterContext<'_>, delta_seconds: f64) {
    ensure_described();
    let model = resolve_model(ctx);
    let cluster = cluster(ctx);
    histogram!(METRIC_TPOT_SECONDS, "model" => model.clone(), "cluster" => cluster.clone()).record(delta_seconds);
}

/// Emit one finish-reason observation.
pub fn record_finish_reason(ctx: &HttpFilterContext<'_>, reason: &str) {
    let model = resolve_model(ctx);
    let cluster = cluster(ctx);
    counter!(
        METRIC_FINISH_REASONS_TOTAL,
        "model" => model.clone(),
        "cluster" => cluster.clone(),
        "reason" => reason.to_owned()
    )
    .increment(1);
}
