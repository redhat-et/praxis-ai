// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Praxis Contributors

use bytes::Bytes;
use praxis_filter::{FilterAction, HttpFilter};
use serde_json::json;

use super::ReasoningEffortMapFilter;

const GATED_MODEL: &str = "Inferact/Qwen3.8-Flash-Next-NVFP4";

fn make_filter_from(yaml: &str) -> Box<dyn HttpFilter> {
    let cfg: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap();
    ReasoningEffortMapFilter::from_config(&cfg).unwrap()
}

/// Default config: gated on the hosted Qwen model with the built-in value map.
fn make_filter() -> Box<dyn HttpFilter> {
    make_filter_from(&format!("models: [{GATED_MODEL}]"))
}

async fn run_with_body(filter: &dyn HttpFilter, json: &serde_json::Value) -> (serde_json::Value, bool) {
    let req = crate::test_utils::make_request(http::Method::POST, "/v1/chat/completions");
    let mut ctx = crate::test_utils::make_filter_context(&req);
    let raw = serde_json::to_vec(json).unwrap();
    let mut body = Some(Bytes::from(raw));

    drop(filter.on_request_body(&mut ctx, &mut body, true).await.unwrap());

    let result: serde_json::Value = serde_json::from_slice(body.as_ref().unwrap()).unwrap();
    let mutated = serde_json::to_vec(&result).unwrap() != serde_json::to_vec(json).unwrap();
    (result, mutated)
}

async fn run(filter: &dyn HttpFilter, json: &serde_json::Value) -> (serde_json::Value, bool) {
    run_with_body(filter, json).await
}

#[tokio::test]
async fn chat_top_level_high_becomes_xhigh() {
    let filter = make_filter();
    let input = json!({
        "model": GATED_MODEL,
        "reasoning_effort": "high",
        "messages": [{"role": "user", "content": "hi"}]
    });

    let (result, mutated) = run(&*filter, &input).await;

    assert!(mutated, "high should be rewritten");
    assert_eq!(result["reasoning_effort"], "xhigh");
    assert_eq!(result["model"], GATED_MODEL);
}

#[tokio::test]
async fn responses_reasoning_effort_high_becomes_xhigh() {
    let filter = make_filter();
    let input = json!({
        "model": GATED_MODEL,
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
    let input = json!({"model": GATED_MODEL, "reasoning_effort": "minimal", "messages": []});

    let (result, mutated) = run(&*filter, &input).await;

    assert!(mutated);
    assert_eq!(result["reasoning_effort"], "low");
}

#[tokio::test]
async fn accepted_values_untouched() {
    let filter = make_filter();
    for accepted in ["xhigh", "medium", "low"] {
        let input = json!({"model": GATED_MODEL, "reasoning_effort": accepted, "messages": []});
        let (result, mutated) = run(&*filter, &input).await;
        assert!(!mutated, "{accepted} is valid and must not be rewritten");
        assert_eq!(result["reasoning_effort"], accepted);
    }
}

#[tokio::test]
async fn unmapped_value_untouched() {
    let filter = make_filter();
    let input = json!({"model": GATED_MODEL, "reasoning_effort": "ultra", "messages": []});

    let (result, mutated) = run(&*filter, &input).await;

    assert!(!mutated, "values outside the map pass through for the backend to judge");
    assert_eq!(result["reasoning_effort"], "ultra");
}

#[tokio::test]
async fn other_model_untouched() {
    let filter = make_filter();
    let input = json!({"model": "gpt-5.6-luna", "reasoning_effort": "high", "messages": []});

    let (result, mutated) = run(&*filter, &input).await;

    assert!(!mutated, "models outside the config must keep the client's effort");
    assert_eq!(result["reasoning_effort"], "high");
}

#[tokio::test]
async fn missing_model_untouched() {
    let filter = make_filter();
    let input = json!({"reasoning_effort": "high", "messages": []});

    let (result, mutated) = run(&*filter, &input).await;

    assert!(!mutated, "a body with no model field matches no configured model");
    assert_eq!(result["reasoning_effort"], "high");
}

#[tokio::test]
async fn non_string_model_untouched() {
    let filter = make_filter();
    for model in [json!(7), json!(null), json!(["x"]), json!({"name": GATED_MODEL})] {
        let input = json!({"model": model, "reasoning_effort": "high", "messages": []});
        let (_result, mutated) = run(&*filter, &input).await;
        assert!(!mutated, "non-string model must not match: {model}");
    }
}

#[tokio::test]
async fn model_match_is_exact() {
    let filter = make_filter();
    for near in [
        format!("{GATED_MODEL}-extra"),
        GATED_MODEL.to_lowercase(),
        "inferact/Qwen3.8-Flash-Next-NVFP4".to_owned(),
        "Inferact/Qwen3.8-Flash-Next-NVFP4 ".to_owned(),
    ] {
        let input = json!({"model": near, "reasoning_effort": "high", "messages": []});
        let (_result, mutated) = run(&*filter, &input).await;
        assert!(!mutated, "model matching is exact, not prefix/case-insensitive: {near}");
    }
}

#[tokio::test]
async fn multiple_models_all_gated() {
    let filter = make_filter_from(&format!("models: [\"other-model\", {GATED_MODEL}]"));
    for model in ["other-model", GATED_MODEL] {
        let input = json!({"model": model, "reasoning_effort": "high", "messages": []});
        let (result, mutated) = run(&*filter, &input).await;
        assert!(mutated, "{model} is in the config and must be rewritten");
        assert_eq!(result["reasoning_effort"], "xhigh");
    }
}

#[tokio::test]
async fn both_shapes_mapped_in_one_body() {
    let filter = make_filter();
    let input = json!({
        "model": GATED_MODEL,
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
        json!({"model": GATED_MODEL, "reasoning_effort": 7, "messages": []}),
        json!({"model": GATED_MODEL, "reasoning_effort": null, "messages": []}),
        json!({"model": GATED_MODEL, "reasoning": "high"}),
        json!({"model": GATED_MODEL, "reasoning": {"effort": null}}),
        json!({"model": GATED_MODEL, "reasoning": {"effort": 3}}),
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
    let input = json!({"model": GATED_MODEL, "messages": [{"role": "user", "content": "hi"}]});

    let (_result, mutated) = run(&*filter, &input).await;

    assert!(!mutated);
}

#[tokio::test]
async fn custom_values_map_respected() {
    let filter = make_filter_from(&format!("models: [{GATED_MODEL}]\nvalues:\n  high: medium\n"));
    let input = json!({"model": GATED_MODEL, "reasoning_effort": "high", "messages": []});

    let (result, mutated) = run(&*filter, &input).await;

    assert!(mutated);
    assert_eq!(result["reasoning_effort"], "medium");
}

#[tokio::test]
async fn custom_values_replace_defaults_not_extend() {
    let filter = make_filter_from(&format!("models: [{GATED_MODEL}]\nvalues:\n  high: medium\n"));
    let input = json!({"model": GATED_MODEL, "reasoning_effort": "minimal", "messages": []});

    let (result, mutated) = run(&*filter, &input).await;

    assert!(
        !mutated,
        "an explicit `values` block is the whole map, defaults included"
    );
    assert_eq!(result["reasoning_effort"], "minimal");
}

#[tokio::test]
async fn identity_mapping_not_counted_as_mutation() {
    let filter = make_filter_from(&format!("models: [{GATED_MODEL}]\nvalues:\n  high: high\n"));
    let input = json!({"model": GATED_MODEL, "reasoning_effort": "high", "messages": []});

    let (_result, mutated) = run(&*filter, &input).await;

    assert!(!mutated);
}

#[tokio::test]
async fn non_json_body_passes_through() {
    let filter = make_filter();
    let req = crate::test_utils::make_request(http::Method::POST, "/v1/chat/completions");
    let mut ctx = crate::test_utils::make_filter_context(&req);
    let original = Bytes::from_static(b"not json at all");
    let mut body = Some(original.clone());

    let action = filter.on_request_body(&mut ctx, &mut body, true).await.unwrap();

    assert!(matches!(action, FilterAction::Continue));
    assert_eq!(body.as_ref().unwrap(), &original, "body must survive untouched");
}

#[tokio::test]
async fn mid_stream_chunk_ignored() {
    let filter = make_filter();
    let req = crate::test_utils::make_request(http::Method::POST, "/v1/chat/completions");
    let mut ctx = crate::test_utils::make_filter_context(&req);
    let input = json!({"model": GATED_MODEL, "reasoning_effort": "high", "messages": []});
    let mut body = Some(Bytes::from(serde_json::to_vec(&input).unwrap()));

    let action = filter.on_request_body(&mut ctx, &mut body, false).await.unwrap();

    assert!(matches!(action, FilterAction::Continue));
    let result: serde_json::Value = serde_json::from_slice(body.as_ref().unwrap()).unwrap();
    assert_eq!(
        result["reasoning_effort"], "high",
        "rewriting happens once on the complete body, not per chunk"
    );
}

#[tokio::test]
async fn empty_models_rejected_at_config() {
    let cfg: serde_yaml::Value = serde_yaml::from_str("models: []").unwrap();
    let err = ReasoningEffortMapFilter::from_config(&cfg)
        .err()
        .expect("an unscoped rewrite must fail at startup");
    assert!(
        err.to_string().contains("models"),
        "error names the offending field: {err}"
    );
}

#[tokio::test]
async fn missing_models_rejected_at_config() {
    let err = ReasoningEffortMapFilter::from_config(&serde_yaml::Value::Null)
        .err()
        .expect("models has no default — silent global rewriting is unsafe");
    assert!(
        err.to_string().contains("models"),
        "error names the offending field: {err}"
    );
}

#[tokio::test]
async fn unknown_field_rejected_at_config() {
    let cfg: serde_yaml::Value = serde_yaml::from_str(&format!("models: [{GATED_MODEL}]\nnot_a_field: 1")).unwrap();
    assert!(ReasoningEffortMapFilter::from_config(&cfg).is_err());
}
