// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 Praxis Contributors

//! Tests for the reject-upgrade example configuration.

use std::collections::HashMap;

use praxis_test_utils::{free_port, http_send, parse_body, parse_status, start_header_echo_backend};

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[test]
fn reject_upgrade_config_parses() {
    let config = super::load_example_config(
        "reject-upgrade.yaml",
        29940,
        HashMap::from([("127.0.0.1:3000", 29941_u16)]),
    );

    assert_eq!(config.listeners.len(), 1, "should have 1 listener");
    assert_eq!(&*config.listeners[0].name, "gateway", "listener name should be gateway");
}

/// A WebSocket handshake must be refused before it reaches the backend, so an
/// upgraded tunnel can never open on a metered route. This is the core
/// guarantee: the response is a 4xx JSON error, never `101 Switching Protocols`.
#[test]
fn reject_upgrade_refuses_websocket_handshake() {
    let backend_guard = start_header_echo_backend();
    let backend_port = backend_guard.port();
    let proxy_port = free_port();

    let config = super::load_example_config(
        "reject-upgrade.yaml",
        proxy_port,
        HashMap::from([("127.0.0.1:3000", backend_port)]),
    );

    let proxy = praxis_test_utils::start_proxy(&config);
    let raw = http_send(
        proxy.addr(),
        "GET /v1/chat/completions HTTP/1.1\r\n\
         Host: localhost\r\n\
         Connection: Upgrade\r\n\
         Upgrade: websocket\r\n\
         Sec-WebSocket-Version: 13\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
    );

    assert!(
        !raw.to_lowercase().contains("101 switching protocols"),
        "upgrade must never be switched to a tunnel: {raw}"
    );
    assert_eq!(parse_status(&raw), 400, "upgrade must be refused with 400: {raw}");
    let body = parse_body(&raw);
    assert!(
        body.contains("upgrade_not_supported"),
        "should return the OpenAI-shaped JSON error body: {body}"
    );
}

/// A normal (non-upgrade) request on the same chain must pass through to the
/// backend untouched — the filter only intercepts upgrade attempts.
#[test]
fn reject_upgrade_passes_normal_requests() {
    let backend_guard = start_header_echo_backend();
    let backend_port = backend_guard.port();
    let proxy_port = free_port();

    let config = super::load_example_config(
        "reject-upgrade.yaml",
        proxy_port,
        HashMap::from([("127.0.0.1:3000", backend_port)]),
    );

    let proxy = praxis_test_utils::start_proxy(&config);
    let raw = http_send(
        proxy.addr(),
        "POST /v1/chat/completions HTTP/1.1\r\n\
         Host: localhost\r\n\
         Content-Type: application/json\r\n\
         Connection: close\r\n\r\n",
    );

    assert_eq!(parse_status(&raw), 200, "a normal request must pass through: {raw}");
}
