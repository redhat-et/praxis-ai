# Red Hat Praxis AI downstream

This repository builds EnMaaS Praxis images from
[`praxis-proxy/ai`](https://github.com/praxis-proxy/ai). Runtime configuration
and credentials belong to
[`redhat-et/pricetag`](https://github.com/redhat-et/pricetag).

## Current upstream update

The November 10 update integrates upstream main at
[`7afa1699`](https://github.com/praxis-proxy/ai/commit/7afa1699e70bcd472d1db4c980b9177ca6f1d57c),
with workspace version 0.10.0 and Praxis core 0.8.0. The local merge preserves
both histories. Its content resolution uses upstream `75e8662d` as the
effective baseline of the previous squash-based synchronization.

Step one is updating the fork and producing branch images with the required
behavior preserved. Step two is retiring custom implementations through
upstream replacements and contributions. The final destination is a fork
that builds pinned, unmodified upstream source.

The image's feature list is explicit because upstream `full` no longer
enables OpenAI Responses/Conversations or PostgreSQL. See
[image publishing](docs/enmaas-image-publishing.md) for the build profile,
manual branch builds, immutable tags, SBOMs, and promotion policy. A branch
build does not promote a production image.

## Retained runtime contracts

| Capability | Current compatibility contract | Upstream direction |
|---|---|---|
| API-key authentication | `api_key_auth` validates MaaS keys and publishes trusted tenant metadata and key IDs. Enabled explicitly in EnMaaS builds. | PPE already implements the HTTP validation mechanism; migrate trusted identity consumers before removing the filter. [Tracking](https://github.com/praxis-proxy/ai/issues/707). |
| Catalog | `model_catalog` projects static or overlay-backed model lists with dialect and credential metadata. | Compare overlay authority with the proposed upstream [catalog](https://github.com/praxis-proxy/ai/pull/1839). |
| QoS and admission | `enmaas-policy-filter` calls Metering directly, caches decisions, applies cluster-scoped class/subject request limits and credentials, and preserves global explicit model/quota denials. | Decompose the contracts into upstream policy, rate limiting, credential selection, and event handling. No complete replacement is assumed. |
| Model routing | Upstream `models[].upstream` selects a cluster. Legacy `models[].provider` continues to set the provider header for existing routers. Mixing both schemas is rejected. Legacy intelligent-route overlay fields remain supported. | Migrate deployed configurations to upstream provider profiles and direct cluster selection. |
| Vertex Anthropic | Upstream `anthropic_messages_to_vertexai_anthropic` is available with its feature. Legacy `vertex` remains registered for deployed configurations and keeps encoding/SSE handling. | Migrate and qualify configurations before removing the legacy translator and temporary deployment header-stripping workaround. |
| Model access and normalization | Existing `model_access`, `content_normalize`, and `reasoning_effort_map` configurations remain available. | Confirm all deployed profiles use upstream alternatives before deletion. |
| Metering authorization | `model_policy_check` and its availability setting preserve model-policy preflight and explicit denial behavior. | Follow the upstream [model-policy preflight](https://github.com/praxis-proxy/ai/pull/1912). |
| Metering credentials | `internal_auth_file` is a compatibility spelling for upstream's watched file credential source. It cannot be combined with `internal_auth_token`. | Prefer `internal_auth_token: {file: /path}` in new configurations. The implementation uses upstream rotation and failure handling. |
| Accounting | Automatic parser selection and provider-specific completion/reasoning normalization remain. Upstream buffered-body accumulation is retained. | Follow [automatic parser selection](https://github.com/praxis-proxy/ai/pull/1911) and contribute remaining normalization cases. |
| Timing and metrics | Preserve actual-output TTFT detection, per-request TPOT, ITL, finish reasons, and empty-completion classification. | Upstream basic metrics do not establish parity with these definitions. TTFT still starts at the original request. |
| Upgrade admission | `reject_upgrade` preserves rejection before body-dependent policies can be bypassed. | Follow [core upgrade admission](https://github.com/praxis-proxy/praxis/issues/1482). |

Upstream GCP key-file authentication and cluster scoping replace the previous
implementation. The EnMaaS crate uses the current shared subrequest client;
the Python policy service is not reintroduced. Preserve configured QoS modes
during the update rather than enabling enforcement as part of synchronization.

## Operational qualification

The current upstream target includes the model-inspection body-limit and
oversized-upload fixes. Buffering routes need consistent request and buffer
limits. Token-capture limits remain separate; validate large JSON, terminal
SSE usage events, response replay, and memory under concurrency before raising
production limits. Legacy provider mappings now inherit pipeline limits by
default and enforce an explicitly configured processing cap with HTTP 413.

Track the team's existing operational requirements alongside this update:

- [Large request/response bodies](https://github.com/redhat-et/EnMaaS-opps/issues/41):
  qualify transport and capture limits together; never interpret missing or
  overflowed usage as a proven zero charge.
- [Client disconnects](https://github.com/redhat-et/EnMaaS-opps/issues/40):
  upload-specific 499 handling does not cover all Pingora disconnect paths.
  Cancellation, logging, and incomplete-stream accounting need separate proof.
- [TTFT and retries](https://github.com/redhat-et/EnMaaS-opps/issues/39):
  distinguish end-to-end latency from successful-attempt latency. The issue
  records a silence expiry of November 19, 2026 at 02:56 UTC; confirm its live
  state and ownership before that deadline.

The upstream update does not claim these operational issues are closed.
Qualification must record the exact image digest and rendered configuration,
with a compatible image/config rollback pair. Remove a runtime patch only when
its replacement is present in the selected upstream revision and the required
behavior has been verified.
