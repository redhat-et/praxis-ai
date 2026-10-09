// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 Praxis Contributors

//! Credential-source resolution and token fetch for [`GcpAdcFilter`].
//!
//! [`fetch`] acquires a token for the [`Metadata`](TokenSource::Metadata)
//! source from the GCE/GKE metadata server. The
//! [`ServiceAccountKey`](TokenSource::ServiceAccountKey) mints a token by
//! signing a short-lived `JWT` assertion and exchanging it at Google's
//! OAuth2 token endpoint.

use std::{path::Path, time::Duration};

use bytes::Bytes;
use http::HeaderValue;
use praxis_ai_apis::{
    callout_target::AddressPolicy,
    subrequest::{SubRequest, SubRequestClient, execute_url},
};
use praxis_filter::FilterError;
use serde::Deserialize;

use super::config::{GcpAdcConfig, GcpAdcSource};

const JWT_BEARER_GRANT: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";
const ASSERTION_LIFETIME: Duration = Duration::from_secs(600);
const GOOGLE_TOKEN_HOST: &str = "oauth2.googleapis.com";

// -----------------------------------------------------------------------------
// TokenSource
// -----------------------------------------------------------------------------

/// Resolved credential source used to fetch a token.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum TokenSource {
    /// GCE/GKE/Cloud Run metadata server.
    Metadata {
        /// Service account email or `default`.
        service_account: String,
    },

    /// Parsed `type: service_account` key file.
    ServiceAccountKey(ServiceAccountKey),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ServiceAccountKey {
    pub client_email: String,
    pub private_key_pem: String,
    pub token_url: String,
    pub loopback_test: bool,
}

// -----------------------------------------------------------------------------
// Fetch
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct TokenEndpointResponse {
    access_token: String,
    expires_in: u64,
}

/// Acquire a token for `source`.
///
/// Kept free of caching concerns so it fits
/// [`TokenCache::get_or_refresh`](praxis_ai_apis::token_cache::TokenCache::get_or_refresh)'s
/// `fetch` closure shape directly.
///
/// # Errors
///
/// Returns [`FilterError`] if the metadata request fails, returns a
/// non-success status, or its body cannot be parsed.
#[cfg(test)]
pub(super) async fn fetch(
    client: &SubRequestClient,
    source: &TokenSource,
    metadata_host: &str,
    scope: &str,
    timeout: Duration,
) -> Result<(HeaderValue, Duration), FilterError> {
    match source {
        TokenSource::Metadata { service_account } => {
            fetch_metadata_token(client, metadata_host, service_account, scope, timeout).await
        },
        TokenSource::ServiceAccountKey(key) => {
            fetch_service_account_token(client, key, scope, timeout, AddressPolicy::AllowPrivate).await
        },
    }
}

/// Acquire a token through `SubRequestClient` targeting the metadata server.
///
/// The metadata protocol intentionally targets a private endpoint. The
/// configured host is separately restricted to Google's metadata hostname.
pub(super) async fn fetch_pinned(
    client: &SubRequestClient,
    source: &TokenSource,
    metadata_host: &str,
    scope: &str,
    timeout: Duration,
) -> Result<(HeaderValue, Duration), FilterError> {
    match source {
        TokenSource::Metadata { service_account } => {
            let url = metadata_token_url(metadata_host, service_account, scope);
            fetch_metadata_token_url(client, &url, timeout, metadata_address_policy(metadata_host)).await
        },
        TokenSource::ServiceAccountKey(key) => {
            let policy = if key.loopback_test { AddressPolicy::AllowPrivate } else { AddressPolicy::PublicOnly };
            fetch_service_account_token(client, key, scope, timeout, policy).await
        },
    }
}

/// Use the production metadata-only policy except for literal loopback mocks
/// compiled into this crate's unit tests.
#[cfg(test)]
fn metadata_address_policy(metadata_host: &str) -> AddressPolicy {
    if metadata_host.split(':').next().unwrap_or(metadata_host) == "127.0.0.1" {
        return AddressPolicy::AllowPrivate;
    }
    AddressPolicy::AllowGoogleMetadata
}

/// Use the metadata-only policy in every production build.
#[cfg(not(test))]
fn metadata_address_policy(_metadata_host: &str) -> AddressPolicy {
    AddressPolicy::AllowGoogleMetadata
}

/// Acquire a token from the GCE/GKE metadata server.
#[cfg(test)]
async fn fetch_metadata_token(
    client: &SubRequestClient,
    metadata_host: &str,
    service_account: &str,
    scope: &str,
    timeout: Duration,
) -> Result<(HeaderValue, Duration), FilterError> {
    let url = metadata_token_url(metadata_host, service_account, scope);
    fetch_metadata_token_url(client, &url, timeout, AddressPolicy::AllowPrivate).await
}

/// Build the metadata token URL from already validated components.
fn metadata_token_url(metadata_host: &str, service_account: &str, scope: &str) -> String {
    let mut url =
        format!("http://{metadata_host}/computeMetadata/v1/instance/service-accounts/{service_account}/token?scopes=");
    url::form_urlencoded::byte_serialize(scope.as_bytes()).for_each(|piece| url.push_str(piece));
    url
}

/// Send one metadata token request via `SubRequestClient`.
async fn fetch_metadata_token_url(
    client: &SubRequestClient,
    url: &str,
    timeout: Duration,
    address_policy: AddressPolicy,
) -> Result<(HeaderValue, Duration), FilterError> {
    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::HeaderName::from_static("metadata-flavor"),
        HeaderValue::from_static("Google"),
    );
    let request = SubRequest {
        method: http::Method::GET,
        uri: http::Uri::default(),
        headers,
        body: Bytes::new(),
    };

    let response = execute_url(client, url, request, 65_536, timeout, address_policy)
        .await
        .map_err(|e| FilterError::from(format!("gcp_adc: metadata token request failed: {e}")))?;

    parse_token_response(response.status, &response.body, "metadata")
}

async fn fetch_service_account_token(
    client: &SubRequestClient,
    key: &ServiceAccountKey,
    scope: &str,
    timeout: Duration,
    address_policy: AddressPolicy,
) -> Result<(HeaderValue, Duration), FilterError> {
    let assertion = sign_assertion(key, scope)?;
    let body = format!(
        "grant_type={JWT_BEARER_GRANT}&assertion={assertion}",
        assertion = url::form_urlencoded::byte_serialize(assertion.as_bytes()).collect::<String>()
    );
    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-www-form-urlencoded"),
    );
    let request = SubRequest {
        method: http::Method::POST,
        uri: http::Uri::default(),
        headers,
        body: Bytes::from(body),
    };
    let response = execute_url(client, &key.token_url, request, 65_536, timeout, address_policy)
        .await
        .map_err(|e| FilterError::from(format!("gcp_adc: service-account token request failed: {e}")))?;
    parse_token_response(response.status, &response.body, "service-account")
}

#[derive(serde::Serialize)]
struct AssertionClaims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: u64,
    exp: u64,
}

fn sign_assertion(key: &ServiceAccountKey, scope: &str) -> Result<String, FilterError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| FilterError::from(format!("gcp_adc: system clock before Unix epoch: {e}")))?
        .as_secs();
    let claims = AssertionClaims {
        iss: &key.client_email,
        scope,
        aud: &key.token_url,
        iat: now,
        exp: now.saturating_add(ASSERTION_LIFETIME.as_secs()),
    };
    let signing_key = jsonwebtoken::EncodingKey::from_rsa_pem(key.private_key_pem.as_bytes())
        .map_err(|e| FilterError::from(format!("gcp_adc: invalid private key in credentials file: {e}")))?;
    jsonwebtoken::encode(&jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256), &claims, &signing_key)
        .map_err(|e| FilterError::from(format!("gcp_adc: failed to sign service-account assertion: {e}")))
}

fn parse_token_response(status_code: u16, body: &[u8], context: &str) -> Result<(HeaderValue, Duration), FilterError> {
    let status = http::StatusCode::from_u16(status_code).unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR);
    if !status.is_success() {
        return Err(FilterError::from(format!("gcp_adc: {context} token endpoint returned HTTP status {status}")));
    }
    let token: TokenEndpointResponse = serde_json::from_slice(body)
        .map_err(|e| FilterError::from(format!("gcp_adc: failed to parse {context} token response: {e}")))?;
    let mut authorization = HeaderValue::from_str(&format!("Bearer {}", token.access_token))
        .map_err(|e| FilterError::from(format!("gcp_adc: {context} token is not a valid header value: {e}")))?;
    authorization.set_sensitive(true);
    Ok((authorization, Duration::from_secs(token.expires_in)))
}


// -----------------------------------------------------------------------------
// GoogleApplicationCredentials
// -----------------------------------------------------------------------------

/// Parse of a Google ADC JSON file.
#[derive(Debug, Deserialize)]
struct GoogleApplicationCredentials {
    /// Google credential `type` field.
    #[serde(rename = "type")]
    cred_type: String,
    #[serde(default)]
    client_email: Option<String>,
    #[serde(default)]
    private_key: Option<String>,
    #[serde(default)]
    token_uri: Option<String>,
}

// -----------------------------------------------------------------------------
// Resolution
// -----------------------------------------------------------------------------

/// Resolve the runtime token source from config and an optional ADC path.
///
/// `application_credentials` is the value of `GOOGLE_APPLICATION_CREDENTIALS`
/// when called from production, or a test-supplied path. It is never read
/// from the environment here so unit tests stay hermetic.
///
/// # Errors
///
/// Returns [`FilterError`] if a configured file is missing, unreadable,
/// or has an unsupported `type`.
pub(super) fn resolve_token_source(
    config: &GcpAdcConfig,
    application_credentials: Option<&Path>,
) -> Result<TokenSource, FilterError> {
    let metadata_source = || TokenSource::Metadata {
        service_account: config.service_account.clone().unwrap_or_else(|| "default".to_owned()),
    };
    match config.source {
        GcpAdcSource::Metadata => Ok(metadata_source()),
        GcpAdcSource::KeyFile => {
            let path = config
                .credentials_file
                .as_deref()
                .ok_or_else(|| FilterError::from("gcp_adc: source key_file requires credentials_file"))?;
            parse_credential_file(Path::new(path))
        },
        GcpAdcSource::Adc => match application_credentials {
            Some(path) => parse_credential_file(path),
            None => Ok(metadata_source()),
        },
    }
}

/// Read a Google ADC JSON file and map its `type` to a [`TokenSource`].
fn parse_credential_file(path: &Path) -> Result<TokenSource, FilterError> {
    let raw = std::fs::read_to_string(path).map_err(|error| {
        FilterError::from(format!(
            "gcp_adc: failed to read credentials file '{}': {error}",
            path.display()
        ))
    })?;
    let parsed: GoogleApplicationCredentials = serde_json::from_str(&raw).map_err(|error| {
        FilterError::from(format!(
            "gcp_adc: failed to parse credentials file '{}': {error}",
            path.display()
        ))
    })?;
    match parsed.cred_type.as_str() {
        "service_account" => service_account_source(parsed),
        "authorized_user" => Err(FilterError::from(
            "gcp_adc: gcloud user ADC (authorized_user) is not supported",
        )),
        "external_account" => Err(FilterError::from(
            "gcp_adc: external_account (WIF/STS) is not implemented yet",
        )),
        other => Err(FilterError::from(format!(
            "gcp_adc: unsupported credential type '{other}'"
        ))),
    }
}

fn service_account_source(parsed: GoogleApplicationCredentials) -> Result<TokenSource, FilterError> {
    let client_email = parsed
        .client_email
        .filter(|value| !value.is_empty())
        .ok_or_else(|| FilterError::from("gcp_adc: credentials file is missing client_email"))?;
    super::config::validate_service_account(&client_email)?;
    let private_key_pem = parsed
        .private_key
        .filter(|value| !value.is_empty())
        .ok_or_else(|| FilterError::from("gcp_adc: credentials file is missing private_key"))?;
    let token_uri = parsed
        .token_uri
        .filter(|value| !value.is_empty())
        .ok_or_else(|| FilterError::from("gcp_adc: credentials file is missing token_uri"))?;
    let token_url = validate_token_uri(&token_uri)?;
    Ok(TokenSource::ServiceAccountKey(ServiceAccountKey {
        client_email,
        private_key_pem,
        loopback_test: token_url.starts_with("http://127.0.0.1"),
        token_url,
    }))
}

fn validate_token_uri(raw: &str) -> Result<String, FilterError> {
    let parsed = url::Url::parse(raw)
        .map_err(|e| FilterError::from(format!("gcp_adc: credentials file token_uri is invalid: {e}")))?;
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err("gcp_adc: credentials file token_uri must not contain credentials, query, or fragment".into());
    }
    let host = parsed.host_str().unwrap_or("");
    let google = parsed.scheme() == "https" && host == GOOGLE_TOKEN_HOST && parsed.path() == "/token";
    let loopback = cfg!(test) && parsed.scheme() == "http" && host == "127.0.0.1";
    if !google && !loopback {
        return Err("gcp_adc: credentials file token_uri must be https://oauth2.googleapis.com/token".into());
    }
    Ok(parsed.to_string())
}
