# Red Hat Praxis AI downstream

This public repository is a GitHub fork of [`praxis-proxy/ai`](https://github.com/praxis-proxy/ai).
Its `main` branch is kept aligned with upstream `main` by the scheduled
`Sync upstream main` workflow. The workflow merges upstream changes into ET
`main` on weekdays and can also be run manually. If a sync conflicts, the
workflow fails rather than force-pushing; resolve the conflict and rerun it.

## Downstream changes

- Keep `main` for upstream sync and fork-maintenance tooling; do not add
  EnMaaS-only filters or deployment configuration directly to it.
- For an EnMaaS release candidate, create a separate integration branch from
  `main` in the maintainer's environment. Keep only the EnMaaS-required filters
  and not-yet-merged upstream PR changes there.
- Submit each generally useful filter to `praxis-proxy/ai` as its own upstream
  PR. Record the upstream PR and commit beside each downstream patch. Once an
  upstream PR merges, sync `main`, remove the duplicate downstream patch, and
  retest the integration branch against the EnMaaS config.
- Build release images only from pushed ET commits. Record the source commit,
  Cargo feature set, and image digest; publish immutable tags for deployment.
- Keep credentials and service-account key material out of this repository.

The EnMaaS deploy manifests and runtime configuration live in
[`redhat-et/pricetag`](https://github.com/redhat-et/pricetag). This repository
owns the Praxis binary and filters; PriceTag deploys the already-built image.

## EnMaaS downstream filter inventory

The current EnMaaS runtime config in `redhat-et/pricetag` uses the filters
below. Upstream status was checked against `praxis-proxy/ai` `main` at
`00489463`. The custom filter source currently lives on the historical
`noyitz/ai:feat/reject-upgrade-filter-adopted` branch; that branch is a source
reference only, not the EnMaaS release source. Yossi's EnMaaS integration
branch should be assembled in the ET repo/environment from reviewed patches.

| Filter / capability | EnMaaS use | Current source path and commit | Upstream status |
|---|---|---|---|
| `api_key_auth` | Validate PriceTag API keys and publish tenant identity | `filters/src/api_key_auth/`; custom commit `a895a2fb` | Absent from upstream `main`; no open PR found |
| `model_access` | Apply model allow/deny policy by tenant group | `filters/src/model_access/`; custom commit `a895a2fb` | Absent from upstream `main`; no open PR found |
| `model_catalog` | Serve the unified Anthropic model catalog | `filters/src/model_catalog/`; custom commit `0984c38e` | Absent from upstream `main`; no open PR found |
| `content_normalize` | Normalize Anthropic content for compatible backends | `filters/src/content_normalize/`; custom commit `6247a5f7` | Absent from upstream `main`; no open PR found |
| `reasoning_effort_map` | Map unsupported reasoning-effort values for selected backends | `filters/src/reasoning_effort_map/`; commits `4f98f89a`, `12a001c0` | Absent from upstream `main`; no open PR found |
| `reject_upgrade` | Reject upgrades that bypass the metered request/response path | `filters/src/reject_upgrade/`; commits `6b6334ff`, `dadca945` | Absent from upstream `main`; no open PR found |
| `stream_usage_inject` | Request usage reporting on OpenAI Chat Completions streams | `filters/src/token_usage/stream_usage.rs`; custom commit `2df8a618` | [PR #1231](https://github.com/praxis-proxy/ai/pull/1231) is open |
| GCP `key_file` token source | Mint short-lived GCP tokens from a mounted SA key | `filters/src/gcp/` changes in the PR | [PR #1356](https://github.com/praxis-proxy/ai/pull/1356) is open; base `gcp_adc` exists upstream |
| Vertex dialect routing | Translate only `vertex/*` models and route them to Vertex | `apis/src/vertex/` changes in the PR | [PR #1358](https://github.com/praxis-proxy/ai/pull/1358) is open |
| StreamBuffer token-count correction | Prevent double-counted JSON chunks from becoming zero-usage events | `filters/src/token_usage/count.rs` and tests in the PR | [PR #1360](https://github.com/praxis-proxy/ai/pull/1360) is open |

`external_metering`, `identity_header_guard`, `model_to_header`,
`token_count`, and `token_usage_headers` are present in upstream `main`.
`credential_injection`, `headers`, `router`, and `load_balancer` come from the
upstream Praxis filter/core stack. The feature-enabled EnMaaS image must include
the downstream-only filters above plus the Vertex PRs, built with
`PRAXIS_AI_FEATURES=full,gcp-adc-filter`.
