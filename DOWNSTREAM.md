# Red Hat Praxis AI downstream

This repository is the ET downstream of
[`praxis-proxy/ai`](https://github.com/praxis-proxy/ai) and is the source for
EnMaaS Praxis images. Runtime configuration and credentials remain in
[`redhat-et/pricetag`](https://github.com/redhat-et/pricetag).

## Purpose of this document

This is the focused EnMaaS capability handoff for the Praxis team. It lists
only the six ET-owned capabilities that still provide functionality not yet
available in the upstream Praxis stack. The goal is to decide, with the Praxis
team, whether each capability should be implemented upstream, replaced by an
existing upstream primitive, or remain an explicitly downstream integration.

The inventory is intentionally not a list of every filter currently present in
the ET image.

## Six remaining EnMaaS capabilities

| Capability | EnMaaS behavior | ET implementation | Upstream tracking | Long-term question |
|---|---|---|---|---|
| `api_key_auth` | Validates MaaS API keys and publishes trusted tenant identity for gateway authorization | `filters/src/api_key_auth/` | [Issue #707](https://github.com/praxis-proxy/ai/issues/707) | Can upstream provide a generic external API-key validator with configurable validation endpoint, identity mapping, caching, and fail-closed behavior? |
| `model_catalog` | Serves credential/dialect-specific `/v1/models` responses from the authoritative routing catalog | `filters/src/model_catalog/` | [Issue #1067](https://github.com/praxis-proxy/ai/issues/1067) | Can upstream own a provider-neutral catalog projection with principal/dialect filtering and reloadable configuration? |
| `reject_upgrade` | Rejects connection upgrades that would bypass body-based authorization, metering, and token accounting | `filters/src/reject_upgrade/` | [Issue #1359](https://github.com/praxis-proxy/ai/issues/1359) | Should Praxis provide a standard policy for rejecting or safely handling upgrades before body-dependent filters? |
| `token_count` provider `auto` | Selects the correct usage parser for mixed OpenAI/Anthropic traffic on shared listeners | `filters/src/token_usage/count.rs` | [Issue #1720](https://github.com/praxis-proxy/ai/issues/1720) | Can upstream expose a documented automatic dialect/parser mode with bounded, fail-safe behavior? |
| File-backed internal metering auth | Reads a Secret-mounted bearer token for private Metering calls and supports rotation without rebuilding the image | `filters/src/metering/` | [Issue #1721](https://github.com/praxis-proxy/ai/issues/1721) | Can the external metering filter support a safe file credential source and rotation semantics? |
| Metering model-policy preflight | Sends the resolved model and tenant to the Metering policy API before provider forwarding; denial becomes HTTP 403 | `filters/src/metering/` | [Issue #1719](https://github.com/praxis-proxy/ai/issues/1719) | Can upstream support an optional authorization preflight distinct from ordinary usage-accounting fail-open behavior? |

## Capabilities deliberately excluded from this six-item handoff

These are not long-term ET gaps for the EnMaaS design:

- `model_access` — replaced by Metering-owned model policy; removal is tracked in
  downstream cleanup [PR #13](https://github.com/redhat-et/praxis-ai/pull/13)
  after every deployed profile migrates.
- `content_normalize` and `reasoning_effort_map` — retired compatibility filters;
  removed by PR #13 after the migration gate.
- `model_to_provider` — replaced in the EnMaaS runtime configuration by
  `intelligent_route` overlays and target-model/provider metadata. The upstream
  model-to-provider work is tracked separately in
  [PR #1698](https://github.com/praxis-proxy/ai/pull/1698); ET should compare it
  with the deployed overlay behavior before adopting it.
- `intelligent_route`, overlay reload, body-authoritative routing, target-model
  rewrites, and routing metadata hardening — these are now upstream Praxis
  capabilities through merged PR #12 and are not ET-only gaps.
- GCP key-file authentication and Vertex dialect translation — upstream work is
  tracked separately and must be compared against the ET image before any
  downstream implementation is removed.

## Migration and removal gate

PR #13 must remain draft until all of the following are true:

1. PriceTag [PR #92](https://github.com/redhat-et/pricetag/pull/92) is merged
   and the dogfood/EnMaaS configuration migration is complete.
2. Every deployed profile, including shadow/green profiles, uses the overlay
   routing/catalog path and has no references to the retired filters.
3. Metering model-policy authorization is enabled and validated with
   `model_policy_check: true` and `model_policy_fail_open: false`.
4. The first static-router to overlay-router transition has completed through
   the documented blue/green cutover with rollback validation.
5. The filter-removal image is built from a pushed ET commit and validated
   before any production cutover.

## Questions for the Praxis team

For each of the six issues above, please confirm:

1. Is the capability appropriate for upstream Praxis, or should it remain an
   ET/platform integration?
2. If upstream-appropriate, what API/configuration contract should ET target?
3. What security, reload, failure-mode, and integration-test requirements must
   be satisfied before ET can remove its implementation?
4. Is there an existing upstream filter or core capability that should replace
   the ET behavior instead?

## Ownership and release rules

- Keep EnMaaS runtime configuration and credentials in `redhat-et/pricetag`.
- Submit generally useful capabilities to `praxis-proxy/ai` as individual PRs.
- Compare upstream behavior and tests before removing any ET implementation.
- Build release images only from pushed commits and record the source commit,
  feature set, and image digest.
- Never commit credentials or service-account key material.

## Current downstream sync

The ET `main` branch is synchronized with upstream through the normal downstream
workflow. Resolve sync conflicts in ET, run the required checks, and record the
resulting sync commit here before building an EnMaaS image.
