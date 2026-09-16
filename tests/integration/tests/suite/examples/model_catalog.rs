// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Praxis Contributors

//! Tests for the model catalog example config.
//!
//! The catalog serves the configured model list at `GET /v1/models` and
//! proxies every other request upstream.

use std::collections::HashMap;

use praxis_test_utils::{free_port, http_get, start_backend_with_shutdown, start_proxy};
use serde_json::Value;

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[test]
fn model_catalog_serves_configured_models() {
    let backend_guard = start_backend_with_shutdown("ok");
    let backend_port = backend_guard.port();
    let proxy_port = free_port();
    let config = super::load_example_config(
        "model-catalog.yaml",
        proxy_port,
        HashMap::from([("127.0.0.1:3000", backend_port)]),
    );
    let proxy = start_proxy(&config);

    let (status, body) = http_get(proxy.addr(), "/v1/models", Some("localhost"));
    assert_eq!(status, 200, "GET /v1/models should be served by the filter");

    let v: Value = serde_json::from_str(&body).unwrap_or_else(|e| panic!("invalid JSON body: {e}: {body}"));
    assert_eq!(v["object"], "list", "OpenAI envelope object should be 'list'");

    let ids: Vec<&str> = v["data"]
        .as_array()
        .expect("data should be an array")
        .iter()
        .map(|m| m["id"].as_str().expect("id should be a string"))
        .collect();

    assert!(
        ids.contains(&"llama-3.1-8b-instruct"),
        "configured model should be listed, got {ids:?}"
    );
    assert!(
        ids.contains(&"mistral-7b-instruct"),
        "configured model should be listed, got {ids:?}"
    );
    assert!(
        ids.contains(&"claude-internal"),
        "configured model should be listed, got {ids:?}"
    );
}

#[test]
fn model_catalog_passes_through_other_requests() {
    let backend_guard = start_backend_with_shutdown("backend-response");
    let backend_port = backend_guard.port();
    let proxy_port = free_port();
    let config = super::load_example_config(
        "model-catalog.yaml",
        proxy_port,
        HashMap::from([("127.0.0.1:3000", backend_port)]),
    );
    let proxy = start_proxy(&config);

    // A GET to a different path is proxied upstream, not intercepted.
    let (status, body) = http_get(proxy.addr(), "/v1/other", Some("localhost"));
    assert_eq!(status, 200, "non-catalog path should be proxied to the backend");
    assert_eq!(body, "backend-response", "non-catalog path should reach the backend");
}
