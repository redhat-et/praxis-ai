# Red Hat Praxis AI downstream

This public repository is a GitHub fork of [`praxis-proxy/ai`](https://github.com/praxis-proxy/ai).
Its `main` branch is the ET build source for EnMaaS Praxis images. Runtime
configuration and credentials remain in [`redhat-et/pricetag`](https://github.com/redhat-et/pricetag).

## Current sync status

Status checked **2026-10-05**:

- ET `main`: `ae9821f8`
- Upstream `praxis-proxy/ai` `main`: `c9cd262f`
- Divergence: 101 commits exist upstream but not in ET; 43 ET commits are not in upstream.
- The weekday `Sync upstream main` workflow is currently failing on merge conflicts. Recent conflicts include Cargo manifests, Vertex/GCP code, metering, filter registration, and integration tests.

The workflow must not force-push. Resolve conflicts in ET, run the full required
checks, and record the resulting sync commit here. Build release images only
from pushed commits and record the source commit, feature set, and image digest.

## Downstream ownership rules

- Keep EnMaaS runtime configuration and all credentials in `redhat-et/pricetag`.
- Submit generally useful filters to `praxis-proxy/ai` as individual upstream PRs.
- When an upstream PR merges, compare behavior and tests before removing the ET
  implementation in a separate cleanup commit.
- Keep credentials and service-account key material out of this repository.

## EnMaaS downstream gap inventory

The table distinguishes ET-only work from capabilities that have an upstream
PR. ET commits are the source-of-record commits on the ET `main` lineage.

| Capability | EnMaaS use | ET implementation | Upstream status |
|---|---|---|---|
| `api_key_auth` | Validate MaaS API keys and publish tenant identity | `070642d1`, `filters/src/api_key_auth/` | No upstream PR |
| `model_access` | Apply group-based model access rules | `070642d1`, `filters/src/model_access/` | No upstream PR |
| `model_catalog` | Serve the unified model catalog | `84e6ebd7`, `filters/src/model_catalog/` | No upstream PR |
| `content_normalize` | Normalize Anthropic content for compatible backends | `101bb873`, `filters/src/content_normalize/` | No upstream PR |
| `reasoning_effort_map` | Map unsupported reasoning values for selected backends | `0fdf5bc7`, `filters/src/reasoning_effort_map/` | No upstream PR |
| `reject_upgrade` | Prevent upgrades from bypassing metering | `a8e735a3`, `ef79aa79`, `filters/src/reject_upgrade/` | No upstream PR |
| `token_count` provider `auto` | Select parsers on mixed-dialect listeners | `f8c74559`, `filters/src/token_usage/count.rs` | No upstream PR |
| StreamBuffer token-count correction | Prevent zero usage from double-delivered JSON bodies | `05b8993a`, `57c4d14e` | [PR #1360](https://github.com/praxis-proxy/ai/pull/1360) open |
| `stream_usage_inject` | Request usage on OpenAI Chat Completions streams | `0c900057` plus follow-up tests/docs | [PR #1231](https://github.com/praxis-proxy/ai/pull/1231) merged; reconcile ET copy before cleanup |
| GCP `key_file` and cluster scope | Mint GCP tokens from a mounted service-account key | `769203cf`, `7f258fb4` | [PR #1356](https://github.com/praxis-proxy/ai/pull/1356) open; upstream has the base `gcp_adc` implementation |
| Vertex dialect routing | Translate `vertex/*` Anthropic rawPredict traffic | `dc338bd8`, `9a5f1b23`, `apis/src/vertex/` | [PR #1358](https://github.com/praxis-proxy/ai/pull/1358) open |
| `model_to_provider` | Map stable client IDs to provider routes and preserve metering identity | `1dfdfdd9`, `814c531b` | [PR #1372](https://github.com/praxis-proxy/ai/pull/1372) open |
| File-backed internal metering auth | Send a Secret-backed bearer token to private metering endpoints | `691677e1`, `filters/src/metering/` | Downstream-only; ET PR [#2](https://github.com/redhat-et/praxis-ai/pull/2) merged |
| User model-policy preflight | Send the resolved model to Metering and fail denied models with 403 | `ae9821f8`, `filters/src/metering/` | Downstream ET PR [#3](https://github.com/redhat-et/praxis-ai/pull/3) merged; no upstream equivalent |

### Upstreamed or partially overlapping capabilities

The current upstream tree contains related/base implementations for
`stream_usage_inject`, GCP ADC/key-file support, and the Vertex Gemini API.
Those are not automatically interchangeable with the ET EnMaaS implementations:
ET uses additional routing, cluster-scope, dialect, metering, and deployment
behavior. Each must be compared before removing the ET code.

## Merged ET PRs

- [PR #1](https://github.com/redhat-et/praxis-ai/pull/1) — Vertex context-management compatibility fix.
- [PR #2](https://github.com/redhat-et/praxis-ai/pull/2) — file-backed internal metering authentication.
- [PR #3](https://github.com/redhat-et/praxis-ai/pull/3) — model-policy preflight integration.

## Deployment boundary

The EnMaaS deploy manifests, listener routes, Secret mounts, and runtime
configuration live in [`redhat-et/pricetag`](https://github.com/redhat-et/pricetag).
This repository owns the Praxis binary and filters; PriceTag deploys the
already-built image. Build EnMaaS images with:

```text
PRAXIS_AI_FEATURES=full,gcp-adc-filter
```
