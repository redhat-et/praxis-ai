// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 Praxis Contributors

//! Functional test for the `stream_usage_inject` example config.
//!
//! Proves the example chain end-to-end: a streaming Chat Completions
//! request that omits `stream_options` is rewritten in transit so the
//! upstream sees `stream_options.include_usage = true` and answers with
//! a final usage chunk that reaches the client intact. Non-streaming
//! requests pass through untouched.

use std::collections::HashMap;

use praxis_test_utils::{
    CapturedRequest, StatefulCapturingBackend, StatefulCapturingGuard, free_port, http_send, json_post,
    load_example_config, parse_body, parse_status, start_proxy,
};

/// Chat Completions SSE stream ending in a usage chunk, as an
/// upstream sends it when `include_usage` was requested.
const SSE_BODY: &str = concat!(
    "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"created\":1700000000,",
    "\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"}}]}\n\n",
    "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"created\":1700000000,",
    "\"model\":\"gpt-4\",\"choices\":[],",
    "\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2,\"total_tokens\":7}}\n\n",
    "data: [DONE]\n\n",
);

/// Non-streaming Chat Completions response body.
const JSON_BODY: &str = concat!(
    r#"{"id":"chatcmpl-2","object":"chat.completion","created":1700000000,"model":"gpt-4","#,
    r#""choices":[{"index":0,"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}],"#,
    r#""usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}}"#,
);

/// Captured Chat Completions POSTs only, so cluster health probes or
/// other housekeeping requests do not confuse the assertions.
fn chat_requests(backend: &StatefulCapturingGuard) -> Vec<CapturedRequest> {
    backend
        .requests()
        .into_iter()
        .filter(|r| r.method == "POST" && r.uri == "/v1/chat/completions")
        .collect()
}

/// A streaming request without `stream_options` reaches the upstream
/// with `stream_options.include_usage = true` injected, and the usage
/// chunk that opt-in buys flows back to the client.
#[test]
fn streaming_request_reaches_upstream_with_include_usage() {
    let backend = StatefulCapturingBackend::new(vec![(200, SSE_BODY.to_owned())]).start_with_shutdown();
    let proxy_port = free_port();
    let config = load_example_config(
        "stream-usage-inject.yaml",
        proxy_port,
        HashMap::from([("127.0.0.1:3000", backend.port())]),
    );
    let proxy = start_proxy(&config);

    let raw = http_send(
        proxy.addr(),
        &json_post(
            "/v1/chat/completions",
            r#"{"model":"gpt-4","stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
        ),
    );
    assert_eq!(parse_status(&raw), 200, "streaming request should return 200");

    let requests = chat_requests(&backend);
    assert_eq!(requests.len(), 1, "backend should see exactly one chat request");
    assert!(
        requests[0].body.contains(r#""include_usage":true"#),
        "upstream request body should carry the injected usage opt-in: {}",
        requests[0].body
    );

    // The stream itself is relayed untouched, usage chunk included:
    // that final chunk is exactly what the opt-in buys the gateway.
    assert!(
        parse_body(&raw).contains("\"total_tokens\":7"),
        "downstream response should carry the usage chunk: {}",
        parse_body(&raw)
    );
}

/// A non-streaming request is a no-op for the filter: the upstream sees
/// the body exactly as the client sent it.
#[test]
fn non_streaming_request_passes_through_unchanged() {
    let backend = StatefulCapturingBackend::new(vec![(200, JSON_BODY.to_owned())]).start_with_shutdown();
    let proxy_port = free_port();
    let config = load_example_config(
        "stream-usage-inject.yaml",
        proxy_port,
        HashMap::from([("127.0.0.1:3000", backend.port())]),
    );
    let proxy = start_proxy(&config);

    let body = r#"{"model":"gpt-4","messages":[{"role":"user","content":"hi"}]}"#;
    let raw = http_send(proxy.addr(), &json_post("/v1/chat/completions", body));
    assert_eq!(parse_status(&raw), 200, "non-streaming request should return 200");

    let requests = chat_requests(&backend);
    assert_eq!(requests.len(), 1, "backend should see exactly one chat request");
    assert_eq!(requests[0].body, body, "non-streaming request body should be untouched");
    assert!(
        !requests[0].body.contains("include_usage"),
        "non-streaming request must not carry a usage opt-in: {}",
        requests[0].body
    );
}
