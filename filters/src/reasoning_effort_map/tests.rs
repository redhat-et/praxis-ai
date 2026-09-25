// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Praxis Contributors

use std::sync::Arc;

use bytes::Bytes;
use praxis_filter::{FilterAction, HttpFilter};
use serde_json::json;

use super::ReasoningEffortMapFilter;

fn make_filter_from(yaml: &str) -> Box<dyn HttpFilter> {
    let cfg: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap();
    ReasoningEffortMapFilter::from_config(&cfg).unwrap()
}

/// Default config: scoped to qwen-flash with the built-in value map.
fn make_filter() -> Box<dyn HttpFilter> {
    make_filter_from("clusters: [qwen-flash]")
}

async fn run_on_cluster(
    filter: &dyn HttpFilter,
    json: &serde_json::Value,
    cluster: Option<&str>,
) -> (serde_json::Value, bool) {
    let req = crate::test_utils::make_request(http::Method::POST, "/v1/chat/completions");
    let mut ctx = crate::test_utils::make_filter_context(&req);
    ctx.cluster = cluster.map(Arc::from);
    let raw = serde_json::to_vec(json).unwrap();
    let mut body = Some(Bytes::from(raw));

    drop(filter.on_request_body(&mut ctx, &mut body, true).await.unwrap());

    let result: serde_json::Value = serde_json::from_slice(body.as_ref().unwrap()).unwrap();
    let mutated = serde_json::to_vec(&result).unwrap() != serde_json::to_vec(json).unwrap();
    (result, mutated)
}

async fn run(filter: &dyn HttpFilter, json: &serde_json::Value) -> (serde_json::Value, bool) {
    run_on_cluster(filter, json, Some("qwen-flash")).await
}

#[tokio::test]
async fn chat_top_level_high_becomes_xhigh() {
    let filter = make_filter();
    let input = json!({
        "model": "Inferact/Qwen3.8-Flash-Next-NVFP4",
        "reasoning_effort": "high",
        "messages": [{"role": "user", "content": "hi"}]
    });

    let (result, mutated) = run(&*filter, &input).await;

    assert!(mutated, "high should be rewritten");
    assert_eq!(result["reasoning_effort"], "xhigh");
    assert_eq!(result["model"], "Inferact/Qwen3.8-Flash-Next-NVFP4");
}

#[tokio::test]
async fn responses_reasoning_effort_high_becomes_xhigh() {
    let filter = make_filter();
    let input = json!({
        "model": "Inferact/Qwen3.8-Flash-Next-NVFP4",
        "reasoning": {"effort": "high", "summary": "auto"},
        "input": "hi"
    });

    let (result, mutated) = run(&*filter, &input).await;

    assert!(mutated, "reasoning.effort should be rewritten");
    assert_eq!(result["reasoning"]["effort"], "xhigh");
    assert_eq!(result["reasoning"]["summary"], "auto", "sibling fields preserved");
}

#[tokio::test]
async fn minimal_becomes_low() {
    let filter = make_filter();
    let input = json!({"reasoning_effort": "minimal", "messages": []});

    let (result, mutated) = run(&*filter, &input).await;

    assert!(mutated);
    assert_eq!(result["reasoning_effort"], "low");
}

#[tokio::test]
async fn accepted_values_untouched() {
    let filter = make_filter();
    for accepted in ["xhigh", "medium", "low"] {
        let input = json!({"reasoning_effort": accepted, "messages": []});
        let (result, mutated) = run(&*filter, &input).await;
        assert!(!mutated, "{accepted} is valid and must not be rewritten");
        assert_eq!(result["reasoning_effort"], accepted);
    }
}

#[tokio::test]
async fn unmapped_value_untouched() {
    let filter = make_filter();
    let input = json!({"reasoning_effort": "ultra", "messages": []});

    let (result, mutated) = run(&*filter, &input).await;

    assert!(!mutated, "values outside the map pass through for the backend to judge");
    assert_eq!(result["reasoning_effort"], "ultra");
}

#[tokio::test]
async fn other_cluster_untouched() {
    let filter = make_filter();
    let input = json!({"model": "gpt-5.6-luna", "reasoning_effort": "high", "messages": []});

    let (result, mutated) = run_on_cluster(&*filter, &input, Some("openai")).await;

    assert!(!mutated, "clusters outside the config must keep the client's effort");
    assert_eq!(result["reasoning_effort"], "high");
}

#[tokio::test]
async fn no_cluster_selected_untouched() {
    let filter = make_filter();
    let input = json!({"reasoning_effort": "high", "messages": []});

    let (result, mutated) = run_on_cluster(&*filter, &input, None).await;

    assert!(!mutated, "an unrouted request matches no configured cluster");
    assert_eq!(result["reasoning_effort"], "high");
}

#[tokio::test]
async fn both_shapes_mapped_in_one_body() {
    let filter = make_filter();
    let input = json!({
        "model": "Inferact/Qwen3.8-Flash-Next-NVFP4",
        "reasoning_effort": "high",
        "reasoning": {"effort": "minimal"},
        "messages": []
    });

    let (result, mutated) = run(&*filter, &input).await;

    assert!(mutated);
    assert_eq!(result["reasoning_effort"], "xhigh");
    assert_eq!(result["reasoning"]["effort"], "low");
}

#[tokio::test]
async fn non_string_effort_ignored() {
    let filter = make_filter();
    let cases = [
        json!({"reasoning_effort": 7, "messages": []}),
        json!({"reasoning_effort": null, "messages": []}),
        json!({"reasoning": "high"}),
        json!({"reasoning": {"effort": null}}),
        json!({"reasoning": {"effort": 3}}),
    ];
    for input in cases {
        let (result, mutated) = run(&*filter, &input).await;
        assert!(!mutated, "non-string effort values must be left alone: {input}");
        assert_eq!(result, input);
    }
}

#[tokio::test]
async fn body_without_effort_untouched() {
    let filter = make_filter();
    let input = json!({"model": "x", "messages": [{"role": "user", "content": "hi"}]});

    let (_result, mutated) = run(&*filter, &input).await;

    assert!(!mutated);
}

#[tokio::test]
async fn custom_values_map_respected() {
    let filter = make_filter_from("clusters: [qwen-flash]\nvalues:\n  high: medium\n");
    let input = json!({"reasoning_effort": "high", "messages": []});

    let (result, mutated) = run(&*filter, &input).await;

    assert!(mutated);
    assert_eq!(result["reasoning_effort"], "medium");
}

#[tokio::test]
async fn custom_values_replace_defaults_not_extend() {
    let filter = make_filter_from("clusters: [qwen-flash]\nvalues:\n  high: medium\n");
    let input = json!({"reasoning_effort": "minimal", "messages": []});

    let (result, mutated) = run(&*filter, &input).await;

    assert!(
        !mutated,
        "an explicit `values` block is the whole map, defaults included"
    );
    assert_eq!(result["reasoning_effort"], "minimal");
}

#[tokio::test]
async fn identity_mapping_not_counted_as_mutation() {
    let filter = make_filter_from("clusters: [qwen-flash]\nvalues:\n  high: high\n");
    let input = json!({"reasoning_effort": "high", "messages": []});

    let (_result, mutated) = run(&*filter, &input).await;

    assert!(!mutated);
}

#[tokio::test]
async fn non_json_body_passes_through() {
    let filter = make_filter();
    let req = crate::test_utils::make_request(http::Method::POST, "/v1/chat/completions");
    let mut ctx = crate::test_utils::make_filter_context(&req);
    ctx.cluster = Some(Arc::from("qwen-flash"));
    let original = Bytes::from_static(b"not json at all");
    let mut body = Some(original.clone());

    let action = filter.on_request_body(&mut ctx, &mut body, true).await.unwrap();

    assert!(matches!(action, FilterAction::Continue));
    assert_eq!(body.as_ref().unwrap(), &original, "body must survive untouched");
}

#[tokio::test]
async fn empty_clusters_rejected_at_config() {
    let cfg: serde_yaml::Value = serde_yaml::from_str("clusters: []").unwrap();
    let err = ReasoningEffortMapFilter::from_config(&cfg)
        .err()
        .expect("an unscoped rewrite must fail at startup");
    assert!(
        err.to_string().contains("clusters"),
        "error names the offending field: {err}"
    );
}

#[tokio::test]
async fn missing_clusters_rejected_at_config() {
    let err = ReasoningEffortMapFilter::from_config(&serde_yaml::Value::Null)
        .err()
        .expect("clusters has no default — silent global rewriting is unsafe");
    assert!(
        err.to_string().contains("clusters"),
        "error names the offending field: {err}"
    );
}

#[tokio::test]
async fn unknown_field_rejected_at_config() {
    let cfg: serde_yaml::Value = serde_yaml::from_str("clusters: [qwen-flash]\nnot_a_field: 1").unwrap();
    assert!(ReasoningEffortMapFilter::from_config(&cfg).is_err());
}
