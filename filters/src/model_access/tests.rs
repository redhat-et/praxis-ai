// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Praxis Contributors

//! Unit tests for the model access filter.

use bytes::Bytes;
use http::Method;
use praxis_filter::FilterAction;

use super::ModelAccessFilter;
use crate::test_utils::{make_filter_context, make_request};

// -----------------------------------------------------------------------------
// Config Tests
// -----------------------------------------------------------------------------

#[test]
fn from_config_denylist() {
    let yaml: serde_yaml::Value = serde_yaml::from_str(
        r#"
mode: denylist
models:
  - "o3-pro"
"#,
    )
    .unwrap();
    let filter = ModelAccessFilter::from_config(&yaml).unwrap();
    assert_eq!(filter.name(), "model_access");
}

#[test]
fn from_config_with_overrides() {
    let yaml: serde_yaml::Value = serde_yaml::from_str(
        r#"
mode: denylist
models:
  - "claude-fable-*"
overrides:
  - groups: ["ai-eng"]
    mode: allowlist
    models: ["*"]
"#,
    )
    .unwrap();
    let filter = ModelAccessFilter::from_config(&yaml).unwrap();
    assert_eq!(filter.name(), "model_access");
}

#[test]
fn from_config_rejects_empty_override_groups() {
    let yaml: serde_yaml::Value = serde_yaml::from_str(
        r#"
mode: denylist
models: ["test"]
overrides:
  - groups: []
    mode: allowlist
    models: ["*"]
"#,
    )
    .unwrap();
    assert!(
        ModelAccessFilter::from_config(&yaml).is_err(),
        "empty groups in override should be rejected"
    );
}

// -----------------------------------------------------------------------------
// Denylist Tests
// -----------------------------------------------------------------------------

#[tokio::test]
async fn denylist_blocks_exact_match() {
    let filter = make_filter_simple("denylist", &["o3-pro"]);
    let action = run_with_model(&*filter, "o3-pro", None).await;
    assert!(
        matches!(action, FilterAction::Reject(_)),
        "denied model should be rejected"
    );
}

#[tokio::test]
async fn denylist_blocks_prefix_match() {
    let filter = make_filter_simple("denylist", &["claude-opus-*"]);
    let action = run_with_model(&*filter, "claude-opus-4-20250514", None).await;
    assert!(
        matches!(action, FilterAction::Reject(_)),
        "prefix-matched model should be rejected"
    );
}

#[tokio::test]
async fn denylist_allows_non_matching() {
    let filter = make_filter_simple("denylist", &["o3-pro", "claude-opus-*"]);
    let action = run_with_model(&*filter, "claude-fable-5", None).await;
    assert!(
        matches!(action, FilterAction::Continue),
        "non-matching model should be allowed"
    );
}

// -----------------------------------------------------------------------------
// Allowlist Tests
// -----------------------------------------------------------------------------

#[tokio::test]
async fn allowlist_allows_exact_match() {
    let filter = make_filter_simple("allowlist", &["gpt-4o", "claude-fable-5"]);
    let action = run_with_model(&*filter, "claude-fable-5", None).await;
    assert!(matches!(action, FilterAction::Continue), "allowed model should pass");
}

#[tokio::test]
async fn allowlist_blocks_non_matching() {
    let filter = make_filter_simple("allowlist", &["gpt-4o"]);
    let action = run_with_model(&*filter, "o3-pro", None).await;
    assert!(
        matches!(action, FilterAction::Reject(_)),
        "non-matching model should be rejected"
    );
}

// -----------------------------------------------------------------------------
// Group Override Tests
// -----------------------------------------------------------------------------

#[tokio::test]
async fn group_override_allows_denied_model() {
    let filter = make_filter_with_overrides("denylist", &["claude-fable-*"], &[("ai-eng", "allowlist", &["*"])]);
    // Without group: fable is denied
    let action = run_with_model(&*filter, "claude-fable-5", None).await;
    assert!(
        matches!(action, FilterAction::Reject(_)),
        "fable should be denied without group"
    );

    // With ai-eng group: fable is allowed
    let action = run_with_model(&*filter, "claude-fable-5", Some("ai-eng")).await;
    assert!(
        matches!(action, FilterAction::Continue),
        "ai-eng group should allow fable"
    );
}

#[tokio::test]
async fn group_override_does_not_affect_other_groups() {
    let filter = make_filter_with_overrides("denylist", &["claude-fable-*"], &[("ai-eng", "allowlist", &["*"])]);
    // rhoai group has no override, falls back to default denylist
    let action = run_with_model(&*filter, "claude-fable-5", Some("rhoai")).await;
    assert!(
        matches!(action, FilterAction::Reject(_)),
        "rhoai group should still be denied fable"
    );
}

#[tokio::test]
async fn group_override_restricts_allowed_model() {
    let filter = make_filter_with_overrides(
        "allowlist",
        &["*"],                                       // default: allow everything
        &[("intern", "allowlist", &["gpt-4o-mini"])], // interns: only gpt-4o-mini
    );
    // Without group: everything allowed
    let action = run_with_model(&*filter, "claude-fable-5", None).await;
    assert!(
        matches!(action, FilterAction::Continue),
        "default should allow everything"
    );

    // Intern group: only gpt-4o-mini
    let action = run_with_model(&*filter, "claude-fable-5", Some("intern")).await;
    assert!(matches!(action, FilterAction::Reject(_)), "intern should not get fable");

    let action = run_with_model(&*filter, "gpt-4o-mini", Some("intern")).await;
    assert!(
        matches!(action, FilterAction::Continue),
        "intern should get gpt-4o-mini"
    );
}

// -----------------------------------------------------------------------------
// Edge Cases
// -----------------------------------------------------------------------------

#[tokio::test]
async fn missing_model_field_allows() {
    let filter = make_filter_simple("denylist", &["o3-pro"]);
    let body = br#"{"messages":[{"role":"user","content":"hi"}]}"#;
    let action = run_with_body(&*filter, body, None).await;
    assert!(
        matches!(action, FilterAction::Continue),
        "missing model field should be allowed"
    );
}

#[tokio::test]
async fn invalid_json_allows() {
    let filter = make_filter_simple("denylist", &["o3-pro"]);
    let body = b"not json at all";
    let action = run_with_body(&*filter, body, None).await;
    assert!(
        matches!(action, FilterAction::Continue),
        "invalid JSON should be allowed"
    );
}

#[tokio::test]
async fn rejection_includes_model_name() {
    let filter = make_filter_simple("denylist", &["o3-pro"]);
    let action = run_with_model(&*filter, "o3-pro", None).await;
    if let FilterAction::Reject(r) = action {
        let body_str = r
            .body
            .map(|b| String::from_utf8_lossy(&b).to_string())
            .unwrap_or_default();
        assert!(
            body_str.contains("o3-pro"),
            "rejection body should include the model name"
        );
    } else {
        panic!("expected rejection");
    }
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn make_filter_simple(mode: &str, models: &[&str]) -> Box<dyn praxis_filter::HttpFilter> {
    let models_yaml: String = models
        .iter()
        .map(|m| format!("  - \"{m}\""))
        .collect::<Vec<_>>()
        .join("\n");
    let yaml_str = format!("mode: {mode}\nmodels:\n{models_yaml}");
    let yaml: serde_yaml::Value = serde_yaml::from_str(&yaml_str).unwrap();
    ModelAccessFilter::from_config(&yaml).unwrap()
}

fn make_filter_with_overrides(
    mode: &str,
    models: &[&str],
    overrides: &[(&str, &str, &[&str])],
) -> Box<dyn praxis_filter::HttpFilter> {
    let models_yaml: String = models
        .iter()
        .map(|m| format!("  - \"{m}\""))
        .collect::<Vec<_>>()
        .join("\n");
    let mut yaml_str = format!("mode: {mode}\nmodels:\n{models_yaml}\noverrides:\n");
    for (group, omode, omodels) in overrides {
        yaml_str.push_str(&format!("  - groups: [\"{group}\"]\n    mode: {omode}\n    models:\n"));
        for m in *omodels {
            yaml_str.push_str(&format!("      - \"{m}\"\n"));
        }
    }
    let yaml: serde_yaml::Value = serde_yaml::from_str(&yaml_str).unwrap();
    ModelAccessFilter::from_config(&yaml).unwrap()
}

async fn run_with_model(filter: &dyn praxis_filter::HttpFilter, model: &str, group: Option<&str>) -> FilterAction {
    let body = format!(
        r#"{{"model":"{}","messages":[{{"role":"user","content":"hi"}}]}}"#,
        model
    );
    run_with_body(filter, body.as_bytes(), group).await
}

async fn run_with_body(filter: &dyn praxis_filter::HttpFilter, body: &[u8], group: Option<&str>) -> FilterAction {
    let req = make_request(Method::POST, "/v1/messages");
    let mut ctx = make_filter_context(&req);
    ctx.buffered_request_body = Some(Bytes::copy_from_slice(body));
    if let Some(g) = group {
        ctx.filter_metadata.insert("x-tenant-group".to_owned(), g.to_owned());
    }
    filter.on_request(&mut ctx).await.unwrap()
}
