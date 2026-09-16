// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Praxis Contributors

//! Unit tests for the model catalog filter.

use http::Method;
use praxis_filter::FilterAction;
use serde_json::Value;

use super::ModelCatalogFilter;
use crate::test_utils::{make_filter_context, make_request};

// -----------------------------------------------------------------------------
// Config Tests
// -----------------------------------------------------------------------------

#[test]
fn from_config_openai_default() {
    let filter = make_filter(
        "
models:
  - id: llama-3.1-8b-instruct
",
    );
    assert_eq!(filter.name(), "model_catalog");
}

#[test]
fn from_config_rejects_empty_models() {
    let yaml: serde_yaml::Value = serde_yaml::from_str("models: []").unwrap();
    assert!(
        ModelCatalogFilter::from_config(&yaml).is_err(),
        "empty models list should be rejected"
    );
}

#[test]
fn from_config_rejects_non_absolute_path() {
    let yaml: serde_yaml::Value = serde_yaml::from_str(
        "
path: v1/models
models:
  - id: m
",
    )
    .unwrap();
    assert!(
        ModelCatalogFilter::from_config(&yaml).is_err(),
        "path without leading '/' should be rejected"
    );
}

#[test]
fn from_config_rejects_unknown_field() {
    let yaml: serde_yaml::Value = serde_yaml::from_str(
        "
models:
  - id: m
bogus: true
",
    )
    .unwrap();
    assert!(
        ModelCatalogFilter::from_config(&yaml).is_err(),
        "unknown top-level field should be rejected"
    );
}

// -----------------------------------------------------------------------------
// Routing Tests
// -----------------------------------------------------------------------------

#[tokio::test]
async fn ignores_non_get() {
    let filter = make_filter("models:\n  - id: m\n");
    let action = run(&*filter, Method::POST, "/v1/models").await;
    assert!(
        matches!(action, FilterAction::Continue),
        "POST should pass through untouched"
    );
}

#[tokio::test]
async fn ignores_other_paths() {
    let filter = make_filter("models:\n  - id: m\n");
    let action = run(&*filter, Method::GET, "/v1/chat/completions").await;
    assert!(
        matches!(action, FilterAction::Continue),
        "other path should pass through"
    );
}

#[tokio::test]
async fn serves_configured_path() {
    let filter = make_filter("path: /models\nmodels:\n  - id: m\n");
    let action = run(&*filter, Method::GET, "/models").await;
    assert!(
        matches!(action, FilterAction::TerminalResponse(_)),
        "configured path should be served"
    );
}

// -----------------------------------------------------------------------------
// OpenAI Envelope Tests
// -----------------------------------------------------------------------------

#[tokio::test]
async fn openai_envelope_shape() {
    let filter = make_filter(
        "
format: openai
models:
  - id: llama-3.1-8b-instruct
    owned_by: acme
    created: 1700000000
",
    );
    let body = terminal_body(run(&*filter, Method::GET, "/v1/models").await);
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["object"], "list");
    assert_eq!(v["data"][0]["id"], "llama-3.1-8b-instruct");
    assert_eq!(v["data"][0]["object"], "model");
    assert_eq!(v["data"][0]["owned_by"], "acme");
    assert_eq!(v["data"][0]["created"], 1_700_000_000);
}

// -----------------------------------------------------------------------------
// Anthropic Envelope Tests
// -----------------------------------------------------------------------------

#[tokio::test]
async fn anthropic_envelope_shape() {
    let filter = make_filter(
        r#"
format: anthropic
models:
  - id: claude-internal
    display_name: "Claude (internal)"
    created: 1700000000
"#,
    );
    let body = terminal_body(run(&*filter, Method::GET, "/v1/models").await);
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["has_more"], false);
    assert_eq!(v["first_id"], "claude-internal");
    assert_eq!(v["last_id"], "claude-internal");
    assert_eq!(v["data"][0]["type"], "model");
    assert_eq!(v["data"][0]["id"], "claude-internal");
    assert_eq!(v["data"][0]["display_name"], "Claude (internal)");
    assert_eq!(v["data"][0]["created_at"], "2023-11-14T22:13:20Z");
}

#[tokio::test]
async fn anthropic_display_name_defaults_to_id() {
    let filter = make_filter("format: anthropic\nmodels:\n  - id: bare-model\n");
    let body = terminal_body(run(&*filter, Method::GET, "/v1/models").await);
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["data"][0]["display_name"], "bare-model");
}

// -----------------------------------------------------------------------------
// Catalog Contents Tests
// -----------------------------------------------------------------------------

#[tokio::test]
async fn lists_all_configured_models_in_order() {
    let filter = make_filter(
        "
models:
  - id: llama-3.1-8b-instruct
  - id: mistral-7b-instruct
  - id: claude-internal
",
    );
    let body = terminal_body(run(&*filter, Method::GET, "/v1/models").await);
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        ids(&v),
        vec!["llama-3.1-8b-instruct", "mistral-7b-instruct", "claude-internal"],
        "all configured models should be listed in declaration order"
    );
}

// -----------------------------------------------------------------------------
// Response Header Tests
// -----------------------------------------------------------------------------

#[tokio::test]
async fn sets_json_content_type() {
    let filter = make_filter("models:\n  - id: m\n");
    let action = run(&*filter, Method::GET, "/v1/models").await;
    let FilterAction::TerminalResponse(resp) = action else {
        panic!("expected terminal response");
    };
    assert_eq!(resp.status, 200);
    assert_eq!(
        resp.headers.get(http::header::CONTENT_TYPE).unwrap(),
        "application/json"
    );
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn make_filter(yaml_body: &str) -> Box<dyn praxis_filter::HttpFilter> {
    let yaml: serde_yaml::Value = serde_yaml::from_str(yaml_body).unwrap();
    ModelCatalogFilter::from_config(&yaml).unwrap()
}

async fn run(filter: &dyn praxis_filter::HttpFilter, method: Method, path: &str) -> FilterAction {
    let req = make_request(method, path);
    let mut ctx = make_filter_context(&req);
    filter.on_request(&mut ctx).await.unwrap()
}

fn terminal_body(action: FilterAction) -> Vec<u8> {
    let FilterAction::TerminalResponse(resp) = action else {
        panic!("expected terminal response");
    };
    resp.body.map(|b| b.to_vec()).unwrap_or_default()
}

fn ids(v: &Value) -> Vec<String> {
    v["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_owned())
        .collect()
}
