// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Praxis Contributors

use bytes::Bytes;
use praxis_filter::{FilterAction, HttpFilter};
use serde_json::json;

use super::ContentNormalizeFilter;

fn make_filter() -> Box<dyn HttpFilter> {
    ContentNormalizeFilter::from_config(&serde_yaml::Value::Null).unwrap()
}

async fn run(filter: &dyn HttpFilter, json: &serde_json::Value) -> (serde_json::Value, bool) {
    let req = crate::test_utils::make_request(http::Method::POST, "/v1/messages");
    let mut ctx = crate::test_utils::make_filter_context(&req);
    let raw = serde_json::to_vec(json).unwrap();
    let original_len = raw.len();
    let mut body = Some(Bytes::from(raw));

    drop(
        filter
            .on_request_body(&mut ctx, &mut body, true)
            .await
            .unwrap(),
    );

    let result: serde_json::Value = serde_json::from_slice(body.as_ref().unwrap()).unwrap();
    let mutated = body.as_ref().unwrap().len() != original_len
        || serde_json::to_vec(&result).unwrap() != serde_json::to_vec(json).unwrap();
    (result, mutated)
}

#[tokio::test]
async fn rewrites_server_tool_use_to_tool_use() {
    let filter = make_filter();
    let input = json!({
        "model": "Qwen/Qwen3.8-27B-FP8",
        "max_tokens": 64,
        "messages": [
            {"role": "user", "content": [
                {"type": "text", "text": "search for it"},
                {"type": "server_tool_use", "id": "toolu_srv", "name": "web_search", "input": {"query": "test"}}
            ]}
        ]
    });

    let (result, mutated) = run(&*filter, &input).await;

    assert!(mutated, "should have mutated the body");
    let blocks = result["messages"][0]["content"].as_array().unwrap();
    assert_eq!(blocks[0]["type"], "text", "text block should be unchanged");
    assert_eq!(blocks[1]["type"], "tool_use", "server_tool_use should become tool_use");
    assert_eq!(blocks[1]["name"], "web_search", "name should be preserved");
    assert_eq!(blocks[1]["id"], "toolu_srv", "id should be preserved");
}

#[tokio::test]
async fn rewrites_server_tool_result_to_tool_result() {
    let filter = make_filter();
    let input = json!({
        "model": "Qwen/Qwen3.8-27B-FP8",
        "max_tokens": 64,
        "messages": [
            {"role": "user", "content": [
                {"type": "server_tool_result", "tool_use_id": "toolu_srv", "content": [
                    {"type": "text", "text": "search results here"}
                ]}
            ]}
        ]
    });

    let (result, mutated) = run(&*filter, &input).await;

    assert!(mutated);
    let block = &result["messages"][0]["content"][0];
    assert_eq!(block["type"], "tool_result");
    assert_eq!(block["tool_use_id"], "toolu_srv");
    assert_eq!(block["content"][0]["text"], "search results here");
}

#[tokio::test]
async fn rewrites_tool_reference_inside_tool_result() {
    let filter = make_filter();
    let input = json!({
        "model": "Qwen/Qwen3.8-27B-FP8",
        "max_tokens": 64,
        "messages": [
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_01", "content": [
                    {"type": "tool_reference", "tool_name": "Read"},
                    {"type": "text", "text": "file contents here"}
                ]}
            ]}
        ]
    });

    let (result, _) = run(&*filter, &input).await;

    let inner = result["messages"][0]["content"][0]["content"].as_array().unwrap();
    assert_eq!(inner[0]["type"], "text", "tool_reference should become text");
    assert_eq!(inner[0]["text"], "[tool_reference: Read]");
    assert_eq!(inner[1]["type"], "text", "existing text should be unchanged");
    assert_eq!(inner[1]["text"], "file contents here");
}

#[tokio::test]
async fn noop_on_supported_types_only() {
    let filter = make_filter();
    let input = json!({
        "model": "Qwen/Qwen3.8-27B-FP8",
        "max_tokens": 64,
        "messages": [
            {"role": "user", "content": "hello"},
            {"role": "assistant", "content": [
                {"type": "thinking", "thinking": "let me think"},
                {"type": "text", "text": "hi there"}
            ]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t1", "content": "result text"}
            ]}
        ]
    });

    let (result, mutated) = run(&*filter, &input).await;

    assert!(!mutated, "all types are supported, no mutation needed");
    assert_eq!(result, input);
}

#[tokio::test]
async fn noop_on_string_content() {
    let filter = make_filter();
    let input = json!({
        "model": "Qwen/Qwen3.8-27B-FP8",
        "max_tokens": 64,
        "messages": [
            {"role": "user", "content": "just a string"},
            {"role": "assistant", "content": "also a string"}
        ]
    });

    let (result, mutated) = run(&*filter, &input).await;

    assert!(!mutated);
    assert_eq!(result, input);
}

#[tokio::test]
async fn noop_on_non_json() {
    let filter = make_filter();
    let req = crate::test_utils::make_request(http::Method::POST, "/v1/messages");
    let mut ctx = crate::test_utils::make_filter_context(&req);
    let mut body = Some(Bytes::from_static(b"not json"));

    let action = filter
        .on_request_body(&mut ctx, &mut body, true)
        .await
        .unwrap();

    assert!(matches!(action, FilterAction::Continue));
    assert_eq!(body.as_ref().unwrap().as_ref(), b"not json");
}

#[tokio::test]
async fn noop_before_end_of_stream() {
    let filter = make_filter();
    let req = crate::test_utils::make_request(http::Method::POST, "/v1/messages");
    let mut ctx = crate::test_utils::make_filter_context(&req);
    let input = json!({"model": "test", "messages": [
        {"role": "user", "content": [{"type": "server_tool_use", "id": "t1", "name": "x", "input": {}}]}
    ]});
    let mut body = Some(Bytes::from(serde_json::to_vec(&input).unwrap()));

    let action = filter
        .on_request_body(&mut ctx, &mut body, false)
        .await
        .unwrap();

    assert!(matches!(action, FilterAction::Continue));
}

#[tokio::test]
async fn handles_unknown_type_gracefully() {
    let filter = make_filter();
    let input = json!({
        "model": "test",
        "max_tokens": 64,
        "messages": [
            {"role": "user", "content": [
                {"type": "some_future_type", "name": "fancy_tool", "data": {"key": "val"}}
            ]}
        ]
    });

    let (result, mutated) = run(&*filter, &input).await;

    assert!(mutated);
    let block = &result["messages"][0]["content"][0];
    assert_eq!(block["type"], "text");
    assert_eq!(block["text"], "[some_future_type: fancy_tool]");
}

#[tokio::test]
async fn handles_no_messages_field() {
    let filter = make_filter();
    let input = json!({"model": "test", "max_tokens": 64});

    let (result, mutated) = run(&*filter, &input).await;

    assert!(!mutated);
    assert_eq!(result, input);
}

#[tokio::test]
async fn preserves_all_fields_on_server_tool_use() {
    let filter = make_filter();
    let input = json!({
        "model": "test",
        "max_tokens": 64,
        "messages": [
            {"role": "user", "content": [
                {"type": "server_tool_use", "id": "toolu_abc", "name": "web_search",
                 "input": {"query": "rust async"}, "server_specific_field": true}
            ]}
        ]
    });

    let (result, _) = run(&*filter, &input).await;

    let block = &result["messages"][0]["content"][0];
    assert_eq!(block["type"], "tool_use");
    assert_eq!(block["id"], "toolu_abc");
    assert_eq!(block["name"], "web_search");
    assert_eq!(block["input"]["query"], "rust async");
    assert_eq!(block["server_specific_field"], true);
}

#[tokio::test]
async fn mixed_supported_and_unsupported_in_same_message() {
    let filter = make_filter();
    let input = json!({
        "model": "test",
        "max_tokens": 64,
        "messages": [
            {"role": "user", "content": [
                {"type": "text", "text": "look at this"},
                {"type": "server_tool_use", "id": "t1", "name": "search", "input": {}},
                {"type": "tool_use", "id": "t2", "name": "Read", "input": {"path": "/tmp"}}
            ]}
        ]
    });

    let (result, mutated) = run(&*filter, &input).await;

    assert!(mutated);
    let blocks = result["messages"][0]["content"].as_array().unwrap();
    assert_eq!(blocks[0]["type"], "text", "text stays text");
    assert_eq!(blocks[1]["type"], "tool_use", "server_tool_use becomes tool_use");
    assert_eq!(blocks[2]["type"], "tool_use", "regular tool_use stays tool_use");
}

#[test]
fn filter_name() {
    let filter = make_filter();
    assert_eq!(filter.name(), "content_normalize");
}

#[test]
fn default_config() {
    let filter = make_filter();
    assert_eq!(filter.request_body_access(), praxis_filter::BodyAccess::ReadWrite);
    assert!(matches!(
        filter.request_body_mode(),
        praxis_filter::BodyMode::StreamBuffer { max_bytes: Some(limit) } if limit > 0
    ));
}
