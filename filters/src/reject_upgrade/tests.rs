// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 Praxis Contributors

//! Unit tests for the reject-upgrade filter.

use http::{
    HeaderValue, Method,
    header::{CONNECTION, UPGRADE},
};
use praxis_filter::FilterAction;

use super::RejectUpgradeFilter;
use crate::test_utils::{make_filter_context, make_request};

// -----------------------------------------------------------------------------
// Config Tests
// -----------------------------------------------------------------------------

#[test]
fn from_config_minimal_defaults() {
    let yaml: serde_yaml::Value = serde_yaml::from_str("{}").unwrap();
    let filter = RejectUpgradeFilter::from_config(&yaml).unwrap();
    assert_eq!(filter.name(), "reject_upgrade", "should produce reject_upgrade filter");
}

#[test]
fn from_config_full() {
    let yaml: serde_yaml::Value = serde_yaml::from_str(
        r#"
status: 426
message: "no websockets here"
protocols:
  - websocket
"#,
    )
    .unwrap();
    let filter = RejectUpgradeFilter::from_config(&yaml).unwrap();
    assert_eq!(filter.name(), "reject_upgrade", "full config should parse");
}

#[test]
fn from_config_rejects_non_error_status() {
    let yaml: serde_yaml::Value = serde_yaml::from_str("status: 200").unwrap();
    match RejectUpgradeFilter::from_config(&yaml) {
        Err(err) => assert!(
            err.to_string().contains("4xx or 5xx"),
            "error should mention status range: {err}"
        ),
        Ok(_) => panic!("a 2xx status should be rejected"),
    }
}

#[test]
fn from_config_rejects_empty_protocol_entry() {
    let yaml: serde_yaml::Value = serde_yaml::from_str(
        r#"
protocols:
  - ""
"#,
    )
    .unwrap();
    assert!(
        RejectUpgradeFilter::from_config(&yaml).is_err(),
        "empty protocol entries should be rejected"
    );
}

#[test]
fn from_config_rejects_unknown_fields() {
    let yaml: serde_yaml::Value = serde_yaml::from_str("bogus_field: true").unwrap();
    assert!(
        RejectUpgradeFilter::from_config(&yaml).is_err(),
        "unknown fields should be rejected"
    );
}

// -----------------------------------------------------------------------------
// Behavior Tests
// -----------------------------------------------------------------------------

/// Assert the action is a rejection carrying the expected status.
fn assert_rejected(action: &FilterAction, expected_status: u16) {
    match action {
        FilterAction::Reject(rejection) => {
            assert_eq!(rejection.status, expected_status, "unexpected rejection status");
        },
        other => panic!("expected Reject({expected_status}), got {other:?}"),
    }
}

#[tokio::test]
async fn rejects_websocket_upgrade_by_default() {
    let yaml: serde_yaml::Value = serde_yaml::from_str("{}").unwrap();
    let filter = RejectUpgradeFilter::from_config(&yaml).unwrap();

    let mut req = make_request(Method::GET, "/v1/responses");
    req.headers.insert(UPGRADE, HeaderValue::from_static("websocket"));
    req.headers.insert(CONNECTION, HeaderValue::from_static("Upgrade"));

    let mut ctx = make_filter_context(&req);
    let action = filter.on_request(&mut ctx).await.unwrap();

    assert_rejected(&action, 400);
}

#[tokio::test]
async fn rejection_body_is_openai_shaped_json() {
    let yaml: serde_yaml::Value = serde_yaml::from_str(r#"message: "no websockets here""#).unwrap();
    let filter = RejectUpgradeFilter::from_config(&yaml).unwrap();

    let mut req = make_request(Method::GET, "/v1/responses");
    req.headers.insert(UPGRADE, HeaderValue::from_static("websocket"));

    let mut ctx = make_filter_context(&req);
    let action = filter.on_request(&mut ctx).await.unwrap();

    let FilterAction::Reject(rejection) = action else {
        panic!("expected a rejection");
    };
    assert!(
        rejection
            .headers
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("content-type") && v == "application/json"),
        "rejection must declare a JSON content-type"
    );
    let body = rejection.body.expect("rejection must carry a body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("body must be valid JSON");
    assert_eq!(json["error"]["message"], "no websockets here");
    assert_eq!(json["error"]["type"], "invalid_request_error");
    assert_eq!(json["error"]["code"], "upgrade_not_supported");
}

#[tokio::test]
async fn rejects_any_upgrade_when_no_protocol_list() {
    let yaml: serde_yaml::Value = serde_yaml::from_str("{}").unwrap();
    let filter = RejectUpgradeFilter::from_config(&yaml).unwrap();

    let mut req = make_request(Method::GET, "/v1/responses");
    req.headers.insert(UPGRADE, HeaderValue::from_static("h2c"));

    let mut ctx = make_filter_context(&req);
    let action = filter.on_request(&mut ctx).await.unwrap();

    assert_rejected(&action, 400);
}

#[tokio::test]
async fn honors_custom_status() {
    let yaml: serde_yaml::Value = serde_yaml::from_str("status: 426").unwrap();
    let filter = RejectUpgradeFilter::from_config(&yaml).unwrap();

    let mut req = make_request(Method::GET, "/v1/responses");
    req.headers.insert(UPGRADE, HeaderValue::from_static("websocket"));

    let mut ctx = make_filter_context(&req);
    let action = filter.on_request(&mut ctx).await.unwrap();

    assert_rejected(&action, 426);
}

#[tokio::test]
async fn passes_through_when_no_upgrade_header() {
    let yaml: serde_yaml::Value = serde_yaml::from_str("{}").unwrap();
    let filter = RejectUpgradeFilter::from_config(&yaml).unwrap();

    let mut req = make_request(Method::POST, "/v1/responses");
    req.headers
        .insert("content-type", HeaderValue::from_static("application/json"));

    let mut ctx = make_filter_context(&req);
    let action = filter.on_request(&mut ctx).await.unwrap();

    assert!(
        matches!(action, FilterAction::Continue),
        "a normal POST must pass through"
    );
}

#[tokio::test]
async fn allow_list_ignores_non_matching_token() {
    let yaml: serde_yaml::Value = serde_yaml::from_str(
        r#"
protocols:
  - websocket
"#,
    )
    .unwrap();
    let filter = RejectUpgradeFilter::from_config(&yaml).unwrap();

    let mut req = make_request(Method::GET, "/some/path");
    req.headers.insert(UPGRADE, HeaderValue::from_static("h2c"));

    let mut ctx = make_filter_context(&req);
    let action = filter.on_request(&mut ctx).await.unwrap();

    assert!(
        matches!(action, FilterAction::Continue),
        "a non-matching upgrade token must pass through when a protocol list is set"
    );
}

#[tokio::test]
async fn allow_list_matches_token_with_version_suffix() {
    let yaml: serde_yaml::Value = serde_yaml::from_str(
        r#"
protocols:
  - websocket
"#,
    )
    .unwrap();
    let filter = RejectUpgradeFilter::from_config(&yaml).unwrap();

    let mut req = make_request(Method::GET, "/v1/responses");
    // Case-insensitive name match, ignoring an optional version suffix.
    req.headers.insert(UPGRADE, HeaderValue::from_static("WebSocket/13"));

    let mut ctx = make_filter_context(&req);
    let action = filter.on_request(&mut ctx).await.unwrap();

    assert_rejected(&action, 400);
}

#[tokio::test]
async fn empty_upgrade_header_passes_through() {
    let yaml: serde_yaml::Value = serde_yaml::from_str("{}").unwrap();
    let filter = RejectUpgradeFilter::from_config(&yaml).unwrap();

    let mut req = make_request(Method::GET, "/v1/responses");
    req.headers.insert(UPGRADE, HeaderValue::from_static("   "));

    let mut ctx = make_filter_context(&req);
    let action = filter.on_request(&mut ctx).await.unwrap();

    assert!(
        matches!(action, FilterAction::Continue),
        "an empty Upgrade header is not an upgrade attempt"
    );
}

#[tokio::test]
async fn non_utf8_upgrade_token_fails_closed() {
    let yaml: serde_yaml::Value = serde_yaml::from_str("{}").unwrap();
    let filter = RejectUpgradeFilter::from_config(&yaml).unwrap();

    let mut req = make_request(Method::GET, "/v1/responses");
    req.headers.insert(
        UPGRADE,
        HeaderValue::from_bytes(&[0x80, 0x81, 0x82]).expect("raw bytes should be valid HeaderValue"),
    );

    let mut ctx = make_filter_context(&req);
    let action = filter.on_request(&mut ctx).await.unwrap();

    assert_rejected(&action, 400);
}
