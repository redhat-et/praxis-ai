# Red Hat Praxis AI downstream

This public repository is a GitHub fork of [`praxis-proxy/ai`](https://github.com/praxis-proxy/ai).
Its `main` branch is kept aligned with upstream `main` by the scheduled
`Sync upstream main` workflow. The workflow merges upstream changes into ET
`main` on weekdays and can also be run manually. If a sync conflicts, the
workflow fails rather than force-pushing; resolve the conflict and rerun it.

## Downstream changes

- `main` contains upstream Praxis plus the ET-owned EnMaaS filter ports listed
  below. The scheduled workflow merges upstream `main` into ET `main`; resolve
  any resulting code conflicts here rather than force-pushing.
- Keep EnMaaS runtime configuration and all credentials in `redhat-et/pricetag`,
  not in this Praxis source repository.
- Submit generally useful filters to `praxis-proxy/ai` as individual upstream
  PRs. Record those PRs here. When an upstream PR merges, verify the upstream
  code is equivalent, then remove the duplicate ET implementation in a separate
  commit.
- Build release images only from pushed ET commits. Record the source commit,
  Cargo feature set, and image digest; publish immutable tags for deployment.
- Keep credentials and service-account key material out of this repository.

The EnMaaS deploy manifests and runtime configuration live in
[`redhat-et/pricetag`](https://github.com/redhat-et/pricetag). This repository
owns the Praxis binary and filters; PriceTag deploys the already-built image.

## EnMaaS downstream filter inventory

The current EnMaaS runtime config in `redhat-et/pricetag` uses the filters
below. ET implementation commits are on this repo's `main`; the ET commit
column is the build source of record. Upstream status was checked against
`praxis-proxy/ai` `main` at `00489463`.

| Filter / capability | EnMaaS use | ET implementation / source path | Upstream PR/status |
|---|---|---|---|
| `api_key_auth` | Validate PriceTag API keys and publish tenant identity | ET `070642d1`; `filters/src/api_key_auth/` | No upstream PR found |
| `model_access` | Apply model allow/deny policy by tenant group | ET `070642d1`; `filters/src/model_access/` | No upstream PR found |
| `reqwest` build dependency | Required by `api_key_auth` regardless of GCP feature selection | ET `7304e73f`; `filters/Cargo.toml` | Downstream build adaptation; no upstream PR |
| `model_catalog` | Serve the unified Anthropic model catalog | ET `84e6ebd7`; `filters/src/model_catalog/` | No upstream PR found |
| `content_normalize` | Normalize Anthropic content for compatible backends | ET `101bb873`; `filters/src/content_normalize/` | No upstream PR found |
| `reasoning_effort_map` | Map unsupported reasoning-effort values for selected backends | ET `0fdf5bc7`; `filters/src/reasoning_effort_map/` | No upstream PR found |
| `token_count` provider `auto` | Select Anthropic/OpenAI parser from request path on the unified listener | ET `f8c74559`; `filters/src/token_usage/count.rs` | No upstream PR found |
| `reject_upgrade` | Reject upgrades that bypass the metered request/response path | ET `a8e735a3`, `ef79aa79`; `filters/src/reject_upgrade/` | No upstream PR found |
| `stream_usage_inject` | Request usage reporting on OpenAI Chat Completions streams | ET `0c900057`, `050611fc`; `filters/src/token_usage/stream_usage.rs` | [PR #1231](https://github.com/praxis-proxy/ai/pull/1231) is open |
| GCP `key_file` token source + cluster scope | Mint short-lived GCP tokens from a mounted SA key, only for the selected Vertex cluster | ET `769203cf`, `7f258fb4`; `filters/src/gcp/` | [PR #1356](https://github.com/praxis-proxy/ai/pull/1356) is open with the cluster-scope fix; base `gcp_adc` exists upstream |
| GCP test-key hygiene | Generate ephemeral RSA material in tests instead of committing a PEM private key | ET `72263b1d`; `filters/src/gcp/tests.rs` | Included in the downstream; matching fixture cleanup is a fixup on open PR #1356 |
| Vertex dialect routing | Translate only `vertex/*` models and route them to Vertex | ET `dc338bd8`, `9a5f1b23`; `apis/src/vertex/` | [PR #1358](https://github.com/praxis-proxy/ai/pull/1358) is open |
| `model_to_provider` | Map stable client IDs to a provider route and provider target model; preserve the public model ID through Vertex JSON/SSE responses | ET `1dfdfdd9`; `filters/src/inference/model_to_provider.rs` | No upstream PR yet |
| StreamBuffer token-count correction | Prevent double-delivered JSON bodies from becoming zero-usage events | ET `05b8993a`, `57c4d14e`; `filters/src/token_usage/count.rs` | [PR #1360](https://github.com/praxis-proxy/ai/pull/1360) is open |

`external_metering`, `identity_header_guard`, `model_to_header`,
`token_count`, and `token_usage_headers` are present in upstream `main`.
`credential_injection`, `headers`, `router`, and `load_balancer` come from the
upstream Praxis filter/core stack. ET `main` also carries generated docs,
examples, functional integration tests, the `reject_upgrade` security
registration, and the SSE integration test coverage; see the feature commits
above. Build EnMaaS images with `PRAXIS_AI_FEATURES=full,gcp-adc-filter`.
