// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 Praxis Contributors

use std::collections::HashMap;
use std::io::Write as _;
use std::sync::Arc;

use http::{HeaderName, Method};
use tempfile::NamedTempFile;

use super::*;
use crate::test_utils::{make_filter_context, make_request};

fn key_file() -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    write!(
        file,
        r#"{{
            "premium": [{{"id": "premium-0", "key": "premium-key"}}],
            "main": [{{"id": "main-0", "key": "main-key"}}]
        }}"#
    )
    .unwrap();
    file
}

fn build(path: &std::path::Path) -> Box<dyn HttpFilter> {
    let yaml: serde_yaml::Value = serde_yaml::from_str(&format!(
        "enabled: true\nkey_file: {:?}\ndefault_key_env: QOS_CRED_TEST_UNSET\n",
        path.to_string_lossy()
    ))
    .unwrap();
    QosCredentialFilter::from_config(&yaml).unwrap()
}

fn context<'a>(req: &'a praxis_filter::Request, class: Option<&str>) -> HttpFilterContext<'a> {
    let mut ctx = make_filter_context(req);
    ctx.cluster = Some(Arc::from("cb-glm"));
    if let Some(class) = class {
        ctx.set_metadata("qos.class", class);
    }
    ctx.set_metadata("qos.subject", "11111111-1111-1111-1111-111111111111");
    ctx
}

fn injected<'a>(ctx: &'a HttpFilterContext<'_>) -> Option<&'a str> {
    ctx.extra_request_headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        .map(|(_, value)| value.as_str())
}

#[tokio::test]
async fn premium_selects_premium_key() {
    let file = key_file();
    let filter = build(file.path());
    let req = make_request(Method::POST, "/v1/chat/completions");
    let mut ctx = context(&req, Some("premium"));
    assert!(matches!(
        filter.on_request(&mut ctx).await.unwrap(),
        FilterAction::Continue
    ));
    assert_eq!(injected(&ctx), Some("Bearer premium-key"));
}

#[tokio::test]
async fn main_selects_main_key() {
    let file = key_file();
    let filter = build(file.path());
    let req = make_request(Method::POST, "/v1/chat/completions");
    let mut ctx = context(&req, Some("main"));
    assert!(matches!(
        filter.on_request(&mut ctx).await.unwrap(),
        FilterAction::Continue
    ));
    assert_eq!(injected(&ctx), Some("Bearer main-key"));
}

#[tokio::test]
async fn ratelimit_aliases_to_main() {
    let file = key_file();
    let filter = build(file.path());
    let req = make_request(Method::POST, "/v1/chat/completions");
    let mut ctx = context(&req, Some("ratelimit"));
    assert!(matches!(
        filter.on_request(&mut ctx).await.unwrap(),
        FilterAction::Continue
    ));
    assert_eq!(injected(&ctx), Some("Bearer main-key"));
}

#[tokio::test]
async fn missing_class_uses_main() {
    let file = key_file();
    let filter = build(file.path());
    let req = make_request(Method::POST, "/v1/chat/completions");
    let mut ctx = context(&req, None);
    assert!(matches!(
        filter.on_request(&mut ctx).await.unwrap(),
        FilterAction::Continue
    ));
    assert_eq!(injected(&ctx), Some("Bearer main-key"));
}

#[tokio::test]
async fn shadow_uses_main_key_even_for_premium() {
    let file = key_file();
    let yaml: serde_yaml::Value = serde_yaml::from_str(&format!(
        "enabled: true\nshadow: true\nkey_file: {:?}\ndefault_key_env: QOS_CRED_TEST_UNSET\n",
        file.path().to_string_lossy()
    ))
    .unwrap();
    let filter = QosCredentialFilter::from_config(&yaml).unwrap();
    let req = make_request(Method::POST, "/v1/chat/completions");
    let mut ctx = context(&req, Some("premium"));
    assert!(matches!(
        filter.on_request(&mut ctx).await.unwrap(),
        FilterAction::Continue
    ));
    assert_eq!(injected(&ctx), Some("Bearer main-key"));
}

#[tokio::test]
async fn non_cbglm_cluster_is_untouched() {
    let file = key_file();
    let filter = build(file.path());
    let req = make_request(Method::POST, "/v1/chat/completions");
    let mut ctx = context(&req, Some("premium"));
    ctx.cluster = Some(Arc::from("openai"));
    assert!(matches!(
        filter.on_request(&mut ctx).await.unwrap(),
        FilterAction::Continue
    ));
    assert!(injected(&ctx).is_none());
}

#[tokio::test]
async fn missing_key_and_default_fails_closed() {
    let mut only_main = NamedTempFile::new().unwrap();
    write!(only_main, r#"{{"main": [{{"id": "main-0", "key": "main-key"}}]}}"#).unwrap();
    let yaml: serde_yaml::Value = serde_yaml::from_str(&format!(
        "enabled: true\nkey_file: {:?}\ndefault_key_env: QOS_CRED_TEST_UNSET\n",
        only_main.path().to_string_lossy()
    ))
    .unwrap();
    let filter = QosCredentialFilter::from_config(&yaml).unwrap();
    let req = make_request(Method::POST, "/v1/chat/completions");
    let mut ctx = context(&req, Some("premium"));
    // A missing class ring falls back to the default key; with no default it
    // must fail closed rather than silently forwarding without a credential.
    assert!(matches!(
        filter.on_request(&mut ctx).await.unwrap(),
        FilterAction::Reject(_)
    ));
}

#[tokio::test]
async fn disabled_filter_injects_default_key() {
    let filter = QosCredentialFilter {
        enabled: false,
        shadow: false,
        clusters: vec!["cb-glm".to_owned()],
        header: HeaderName::from_static("authorization"),
        class_metadata: "qos.class".to_owned(),
        subject_metadata: "qos.subject".to_owned(),
        default_class: "main".to_owned(),
        class_aliases: HashMap::new(),
        keys: HashMap::new(),
        default_key: Some("Bearer default-key".to_owned()),
    };
    let req = make_request(Method::POST, "/v1/chat/completions");
    let mut ctx = context(&req, Some("premium"));
    assert!(matches!(
        filter.on_request(&mut ctx).await.unwrap(),
        FilterAction::Continue
    ));
    assert_eq!(injected(&ctx), Some("Bearer default-key"));
}
