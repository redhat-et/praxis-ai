// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Praxis Contributors

use bytes::Bytes;
use praxis_filter::{FilterAction, HttpFilter};
use serde_json::json;

use super::StreamUsageInjectFilter;

fn make_filter() -> Box<dyn HttpFilter> {
    StreamUsageInjectFilter::from_config(&serde_yaml::Value::Null).unwrap()
}

async fn run(filter: &dyn HttpFilter, json: &serde_json::Value) -> (serde_json::Value, bool) {
    let req = crate::test_utils::make_request(http::Method::POST, "/v1/chat/completions");
    let mut ctx = crate::test_utils::make_filter_context(&req);
    let raw = serde_json::to_vec(json).unwrap();
    let mut body = Some(Bytes::from(raw));

    let action = filter.on_request_body(&mut ctx, &mut body, true).await.unwrap();
    let mutated = matches!(action, FilterAction::Continue);

    let result: serde_json::Value =
        serde_json::from_slice(body.as_ref().unwrap()).unwrap();
    (result, mutated)
}

#[tokio::test]
async fn injects_when_streaming_without_stream_options() {
    let filter = make_filter();
    let input = json!({
        "model": "gpt-5.4-mini",
        "stream": true,
        "messages": [{"role": "user", "content": "hi"}]
    });

    let (result, _) = run(&*filter, &input).await;

    assert_eq!(
        result["stream_options"]["include_usage"],
        json!(true),
        "should inject include_usage"
    );
    assert_eq!(result["model"], "gpt-5.4-mini", "other fields preserved");
    assert_eq!(result["stream"], true, "stream field preserved");
}

#[tokio::test]
async fn noop_when_already_present() {
    let filter = make_filter();
    let input = json!({
        "model": "gpt-5.4",
        "stream": true,
        "stream_options": {"include_usage": true},
        "messages": [{"role": "user", "content": "hi"}]
    });

    let (result, _) = run(&*filter, &input).await;

    assert_eq!(
        result["stream_options"]["include_usage"],
        json!(true),
        "should keep existing include_usage"
    );
}

#[tokio::test]
async fn noop_when_not_streaming() {
    let filter = make_filter();
    let input = json!({
        "model": "gpt-5.4",
        "stream": false,
        "messages": [{"role": "user", "content": "hi"}]
    });

    let (result, _) = run(&*filter, &input).await;

    assert!(
        result.get("stream_options").is_none(),
        "should not inject stream_options for non-streaming requests"
    );
}

#[tokio::test]
async fn noop_when_stream_absent() {
    let filter = make_filter();
    let input = json!({
        "model": "gpt-5.4",
        "messages": [{"role": "user", "content": "hi"}]
    });

    let (result, _) = run(&*filter, &input).await;

    assert!(
        result.get("stream_options").is_none(),
        "should not inject when stream field is absent"
    );
}

#[tokio::test]
async fn preserves_existing_stream_options_fields() {
    let filter = make_filter();
    let input = json!({
        "model": "gpt-5.4",
        "stream": true,
        "stream_options": {"include_usage": false, "continuous": true},
        "messages": [{"role": "user", "content": "hi"}]
    });

    let (result, _) = run(&*filter, &input).await;

    assert_eq!(
        result["stream_options"]["include_usage"],
        json!(true),
        "should override include_usage to true"
    );
    assert_eq!(
        result["stream_options"]["continuous"],
        json!(true),
        "should preserve other stream_options fields"
    );
}

#[tokio::test]
async fn gracefully_handles_non_json() {
    let filter = make_filter();
    let req = crate::test_utils::make_request(http::Method::POST, "/v1/chat/completions");
    let mut ctx = crate::test_utils::make_filter_context(&req);
    let mut body = Some(Bytes::from_static(b"not json at all"));

    let action = filter
        .on_request_body(&mut ctx, &mut body, true)
        .await
        .unwrap();

    assert!(
        matches!(action, FilterAction::Continue),
        "should continue on non-JSON body"
    );
    assert_eq!(
        body.as_ref().unwrap().as_ref(),
        b"not json at all",
        "body should be unchanged"
    );
}

#[tokio::test]
async fn noop_before_end_of_stream() {
    let filter = make_filter();
    let req = crate::test_utils::make_request(http::Method::POST, "/v1/chat/completions");
    let mut ctx = crate::test_utils::make_filter_context(&req);
    let input = json!({"model": "gpt-5.4", "stream": true});
    let mut body = Some(Bytes::from(serde_json::to_vec(&input).unwrap()));

    let action = filter
        .on_request_body(&mut ctx, &mut body, false)
        .await
        .unwrap();

    assert!(
        matches!(action, FilterAction::Continue),
        "should continue before end of stream"
    );
}

#[tokio::test]
async fn noop_on_responses_api() {
    let filter = make_filter();
    let req = crate::test_utils::make_request(http::Method::POST, "/v1/responses");
    let mut ctx = crate::test_utils::make_filter_context(&req);
    let input = serde_json::json!({"model": "gpt-4.1", "stream": true, "input": "hi"});
    let mut body = Some(Bytes::from(serde_json::to_vec(&input).unwrap()));

    let action = filter
        .on_request_body(&mut ctx, &mut body, true)
        .await
        .unwrap();

    assert!(matches!(action, FilterAction::Continue));
    let result: serde_json::Value =
        serde_json::from_slice(body.as_ref().unwrap()).unwrap();
    assert!(
        result.get("stream_options").is_none(),
        "should NOT inject stream_options on /v1/responses"
    );
}

#[test]
fn filter_name() {
    let filter = make_filter();
    assert_eq!(filter.name(), "stream_usage_inject");
}

#[test]
fn default_config() {
    let filter = make_filter();
    assert_eq!(
        filter.request_body_access(),
        praxis_filter::BodyAccess::ReadWrite,
        "should request read-write body access"
    );
    assert!(
        matches!(
            filter.request_body_mode(),
            praxis_filter::BodyMode::StreamBuffer { max_bytes: Some(limit) } if limit > 0
        ),
        "should use StreamBuffer mode"
    );
}
