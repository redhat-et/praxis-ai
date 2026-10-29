// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 Praxis Contributors

use std::sync::Arc;

use http::Method;

use super::*;
use crate::test_utils::{make_filter_context, make_request};

fn build(yaml: &str) -> Box<dyn HttpFilter> {
    let config: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap();
    QosEnforcerFilter::from_config(&config).unwrap()
}

fn context<'a>(req: &'a praxis_filter::Request, class: Option<&str>, subject: &str) -> HttpFilterContext<'a> {
    let mut ctx = make_filter_context(req);
    ctx.cluster = Some(Arc::from("cb-glm"));
    if let Some(class) = class {
        ctx.set_metadata("qos.class", class);
    }
    ctx.set_metadata("qos.subject", subject);
    ctx
}

#[test]
fn config_defaults_parse() {
    let filter = build("enabled: true\n");
    assert_eq!(filter.name(), "qos_enforcer");
}

#[test]
fn config_rejects_invalid_bucket() {
    let config: serde_yaml::Value =
        serde_yaml::from_str("enabled: true\nmain:\n  burst: 10\n  refill_per_second: 0\n").unwrap();
    assert!(QosEnforcerFilter::from_config(&config).is_err());
}

#[tokio::test]
async fn hard_mode_denies_after_main_burst() {
    let filter = build("enabled: true\nenforcement: hard\nmain:\n  burst: 1\n  refill_per_second: 0.001\n");
    let req = make_request(Method::POST, "/v1/chat/completions");
    let mut ctx = context(&req, Some("main"), "subject-1");
    assert!(matches!(
        filter.on_request(&mut ctx).await.unwrap(),
        FilterAction::Continue
    ));

    let req = make_request(Method::POST, "/v1/chat/completions");
    let mut ctx = context(&req, Some("main"), "subject-1");
    let rejection = match filter.on_request(&mut ctx).await.unwrap() {
        FilterAction::Reject(rejection) => rejection,
        other => panic!("expected rejection, got {other:?}"),
    };
    assert_eq!(rejection.status, 429);
    assert!(
        rejection
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("retry-after"))
    );
    assert!(
        rejection
            .headers
            .iter()
            .any(|(name, value)| name.eq_ignore_ascii_case("x-enmaas-qos-class") && value == "main")
    );
}

#[tokio::test]
async fn hard_mode_rejects_ratelimit_before_bucket() {
    let filter = build("enabled: true\nenforcement: hard\n");
    let req = make_request(Method::POST, "/v1/chat/completions");
    let mut ctx = context(&req, Some("ratelimit"), "subject-1");
    let rejection = match filter.on_request(&mut ctx).await.unwrap() {
        FilterAction::Reject(rejection) => rejection,
        other => panic!("expected rejection, got {other:?}"),
    };
    assert_eq!(rejection.status, 429);
}

#[tokio::test]
async fn soft_mode_never_rejects() {
    let filter = build("enabled: true\nenforcement: soft\nmain:\n  burst: 1\n  refill_per_second: 0.001\n");
    for _ in 0..3 {
        let req = make_request(Method::POST, "/v1/chat/completions");
        let mut ctx = context(&req, Some("main"), "subject-1");
        assert!(matches!(
            filter.on_request(&mut ctx).await.unwrap(),
            FilterAction::Continue
        ));
    }
    let req = make_request(Method::POST, "/v1/chat/completions");
    let mut ctx = context(&req, Some("ratelimit"), "subject-1");
    assert!(matches!(
        filter.on_request(&mut ctx).await.unwrap(),
        FilterAction::Continue
    ));
}

#[tokio::test]
async fn off_mode_is_a_noop() {
    let filter = build("enabled: true\nenforcement: off\nmain:\n  burst: 1\n  refill_per_second: 0.001\n");
    for _ in 0..3 {
        let req = make_request(Method::POST, "/v1/chat/completions");
        let mut ctx = context(&req, Some("main"), "subject-1");
        assert!(matches!(
            filter.on_request(&mut ctx).await.unwrap(),
            FilterAction::Continue
        ));
    }
}

#[tokio::test]
async fn non_cbglm_cluster_is_not_enforced() {
    let filter = build("enabled: true\nenforcement: hard\nmain:\n  burst: 1\n  refill_per_second: 0.001\n");
    for _ in 0..3 {
        let req = make_request(Method::POST, "/v1/chat/completions");
        let mut ctx = context(&req, Some("main"), "subject-1");
        ctx.cluster = Some(Arc::from("openai"));
        assert!(matches!(
            filter.on_request(&mut ctx).await.unwrap(),
            FilterAction::Continue
        ));
    }
}

#[tokio::test]
async fn premium_bucket_is_disabled_by_default() {
    let filter = build("enabled: true\nenforcement: hard\nmain:\n  burst: 1\n  refill_per_second: 0.001\n");
    for _ in 0..3 {
        let req = make_request(Method::POST, "/v1/chat/completions");
        let mut ctx = context(&req, Some("premium"), "subject-1");
        assert!(matches!(
            filter.on_request(&mut ctx).await.unwrap(),
            FilterAction::Continue
        ));
    }
}

#[tokio::test]
async fn missing_class_defaults_to_main() {
    let filter = build("enabled: true\nenforcement: hard\nmain:\n  burst: 1\n  refill_per_second: 0.001\n");
    let req = make_request(Method::POST, "/v1/chat/completions");
    let mut ctx = context(&req, None, "subject-1");
    assert!(matches!(
        filter.on_request(&mut ctx).await.unwrap(),
        FilterAction::Continue
    ));

    let req = make_request(Method::POST, "/v1/chat/completions");
    let mut ctx = context(&req, None, "subject-1");
    assert!(matches!(
        filter.on_request(&mut ctx).await.unwrap(),
        FilterAction::Reject(_)
    ));
}
