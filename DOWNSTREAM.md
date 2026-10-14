# Red Hat Praxis AI downstream gap inventory

This repository is the EnMaaS/PriceTag downstream of
https://github.com/praxis-proxy/ai. It owns the ET-specific Praxis AI filters
and the build source for EnMaaS Praxis images. Runtime configuration, manifests,
Secret mounts, and credentials remain in
https://github.com/redhat-et/pricetag.

**Snapshot date:** 2026-10-14
**ET main:** `99353a6bf8ef0fc378bc637f9fd66d78c65f1895` (PR #11)
**Upstream main:** `d5b05be0fb59c1973c21e0b7a9ed779462fb0321` (0.7.4)
**ET base:** Praxis crates 0.7.3; ET is not yet rebased onto upstream 0.7.4
**Divergence:** 47 ET-only commits and 175 upstream-only commits at this snapshot

This is a point-in-time compatibility inventory, not a claim that ET `main` is
fully rebased onto the latest upstream. Re-run the comparison after every
upstream sync or Praxis crate bump.

## 1. What Yossi's rebase delivered

### PR #9 — merged

https://github.com/redhat-et/praxis-ai/pull/9

Merge commit `065bd1ae` synchronized the ET fork with the upstream history used
for the EnMaaS port and re-applied the required runtime filters. It preserved
the ET-only filters and ported the reviewed behavior for:

- API-key authentication and tenant identity metadata
- group/model access policy and model catalog
- API compatibility filters that were later retired from the EnMaaS target
- upgrade rejection on unmetered connection upgrades
- GCP cluster-scoped credentials and Vertex Anthropic routing
- model-to-provider routing and PriceTag metering integration
- generated documentation, examples, and integration coverage

It also reconciled ET copies with upstream implementations for the shared
filters instead of treating every ET file as a permanent fork.

### PR #10 — merged

https://github.com/redhat-et/praxis-ai/pull/10

Merge commit `0b35bf3e` completed the post-rebase clippy fixes and corrected the
GCP key-file integration fixture. It introduced no new runtime capability.

The rebase is therefore functionally complete for the reviewed ET snapshot,
but it is not current with upstream 0.7.4 yet. The next sync must be treated as
a new compatibility exercise, especially around the Praxis core API, Anthropic
request/translation changes, Responses changes, token-rate-limit changes, and
the Vertex implementation.

## 2. Current EnMaaS runtime inventory

The active PriceTag configuration uses these ET filters. The source of truth for
whether a filter is actually enabled is the current PriceTag deployment
configuration, not this source repository alone.

| Capability | EnMaaS use | ET implementation | Current upstream status | Decision |
|---|---|---|---|---|
| `api_key_auth` | Validate MaaS API keys and publish tenant identity | `filters/src/api_key_auth/` | No equivalent in upstream main | Keep ET-only |
| `model_access` (follow-up removal) | Enforce group/model access policy | `filters/src/model_access/` | No equivalent in upstream main | **Retained in PR #12 for backward-compatible blue/green rollout.** Group filtering is inert with a single `ge` tenant group, and per-model policy is owned by metering. Remove only in the follow-up cleanup PR after every profile/config/shadow has migrated and the green image is validated. |
| `content_normalize` (follow-up removal) | Legacy provider content normalization | `filters/src/content_normalize/` | No equivalent in upstream main | **Retained in PR #12 for live EnMaaS/shadow config compatibility.** Qwen is retired; remove only in the post-migration cleanup PR. |
| `reasoning_effort_map` (follow-up removal) | Legacy Qwen reasoning compatibility | `filters/src/reasoning_effort_map/` | No equivalent in upstream main | **Retained in PR #12 for config compatibility.** Qwen is retired; remove only in the post-migration cleanup PR. |
| `model_catalog` (overlay-driven) | Serve the governed `/v1/models` catalog as a **projection of the routing overlay** (single source of truth with routing) | `filters/src/model_catalog/` + overlay-source mode | No equivalent in upstream main | Keep ET-only. Discovery membership comes from each overlay candidate's `catalog` field (`openai`/`anthropic`), not a separate hand list, so `/v1/models` cannot drift from what routes. Static `models:` mode retained for back-compat until all profiles are overlay-driven. |
| `reject_upgrade` | Prevent WebSocket/upgrade traffic bypassing metering | `filters/src/reject_upgrade/` | No equivalent in upstream main | Keep ET-only |
| `stream_usage_inject` | Request usage on OpenAI Chat streaming | Upstream implementation retained in ET | Upstream PR #1231 merged | Covered; do not carry a duplicate ET port |
| `model_to_provider` (retire after EnMaaS overlay migration) | Preserve public model identity while selecting provider/target model | `filters/src/inference/model_to_provider.rs` | Upstream PR #1372 closed; superseded downstream by the `intelligent_route` target-model extension | **Decision: retire** once both dogfood and EnMaaS route via `intelligent_route` + overlay and no rendered profile references `filter: model_to_provider`. Remove the filter, its tests/docs, and the EnMaaS `vertex-fragments/*model-to-provider.yaml` in the same change. |
| `intelligent_route` target-model/catalog extension | Select hot-reloaded candidates, rewrite provider target IDs, publish provider markers for dialect/path filters, and carry discovery metadata (`catalog`/`display_name`/`owned_by`) so one overlay is the single source for both routing and `/v1/models` | `filters/src/routing/{intelligent_route,descriptor,overlay}.rs` + `filters/src/model_catalog/` overlay mode | Base overlay routing exists upstream; target-model/body-rewrite + overlay-catalog extension is ET PR #12 | Keep the ET extension for the EnMaaS target image; propose upstream after review |
| Vertex Anthropic dialect | Translate `vertex/*` Anthropic rawPredict requests/responses | `apis/src/vertex/` plus `vertex-anthropic.yaml` | Upstream PR #1358 closed; upstream has Vertex Gemini, not this dialect | Keep ET-only and revalidate on every upstream Vertex change |
| GCP `key_file` | Mint OAuth tokens from a mounted service-account JSON key | `filters/src/gcp/` | Upstream has the `key_file` config shape, but the current upstream fetch path is still not implemented; PR #1356 closed | Keep ET implementation; do not treat upstream config parsing as runtime parity |
| GCP cluster scope | Prevent GCP credentials reaching non-GCP providers | `gcp_adc.clusters` | No upstream equivalent in the current tree | Keep ET-only |
| StreamBuffer token-count correction | Avoid double-feeding the EOS buffer and recording zero usage | ET token-count history | Upstream PR #1360 closed; upstream has related token-count changes but not verified equivalent | Keep the ET behavior until an explicit code/test comparison proves parity |
| File-backed internal metering auth | Authenticate private entitlement/event callouts | ET metering changes plus PriceTag Secret mount | No upstream equivalent | Keep ET-only; paired with PriceTag metering auth |
| Model-policy preflight | Send resolved model to PriceTag and map policy denial to 403; the planned sole model-authorization gate after `model_access` removal | ET metering changes (merged ET PR #3) plus the new `external_metering.model_policy_fail_open` flag | No upstream equivalent | Keep ET-only and deploy only with the matching PriceTag API. Configure `model_policy_check: true` + `model_policy_fail_open: false` on the inference chains so the policy decision fails closed without coupling billing availability to it. |
| `external_metering` scoped policy fail mode | Fail the model-policy decision closed on metering outage without failing the whole callout closed | `external_metering.model_policy_fail_open` (default `true`) | No upstream equivalent | Keep ET-only; propose upstream. **Interim gap:** if the PriceTag metering policy capability is disabled or does not return `model_allowed`, there is no strong per-model gate after `model_access` removal — close this when the metering capability lands. |

### Shared upstream capabilities already present in ET

The ET tree also contains upstream/shared filters that should not be counted as
ET gaps: `external_metering`, `identity_header_guard`, `model_to_header`,
`token_count`, `token_usage_headers`, `credential_injection`, `headers`,
`router`, and `load_balancer`. Their behavior still needs EnMaaS integration
tests, but they are not missing merely because they appear in the ET tree.

## 3. Upstream PR status that changes the old map

| Upstream PR | Current status | Effect on this inventory |
|---|---|---|
| https://github.com/praxis-proxy/ai/pull/1231 | Merged 2026-09-28 | `stream_usage_inject` is upstream-covered; no ET duplicate should be created |
| https://github.com/praxis-proxy/ai/pull/1356 | Closed 2026-10-10, not merged | GCP key-file and cluster-scope behavior remains an ET gap/port |
| https://github.com/praxis-proxy/ai/pull/1358 | Closed 2026-10-10, not merged | Vertex Anthropic rawPredict remains ET-only; upstream Vertex Gemini is a different capability |
| https://github.com/praxis-proxy/ai/pull/1360 | Closed 2026-10-10, not merged | ET StreamBuffer correction needs an explicit upstream comparison before cleanup |
| https://github.com/praxis-proxy/ai/pull/1372 | Closed 2026-10-10, not merged | `model_to_provider` remains ET-only |
| https://github.com/praxis-proxy/ai/pull/1591 | Open | Upstream translation support matrix may become the source for rechecking C1/API dialect coverage |
| https://github.com/praxis-proxy/ai/pull/1662 | Open | Upstream `intelligent_route` observability/overlay validation is relevant to the ET routing overlay |
| https://github.com/praxis-proxy/ai/pull/1588 | Open | FIPS readiness work does not replace the ET runtime filter gaps |

The upstream main tree currently contains Vertex Gemini support at
`apis/src/vertex/gemini/`, but not the ET Vertex Anthropic rawPredict path. It
also contains the shared `gcp_adc` filter and the `key_file` configuration
shape, but its current key-file token-fetch path is not implemented and it has
no ET cluster-gating contract.

## 4. Rebase and release rules

1. Fetch upstream and ET, record both commit IDs, crate versions, and the
   left/right commit counts.
2. Compare every ET-only filter against the upstream registry, docs, examples,
   and integration tests before deciding that it is still a gap.
3. For an upstreamed capability, compare wire/config behavior and tests before
   removing the ET implementation. A similarly named filter is not enough.
4. Keep ET-only filters and their tests in feature-specific commits so a future
   rebase can replay or drop them independently.
5. Build EnMaaS images only from pushed ET commits with
   `PRAXIS_AI_FEATURES=full,gcp-adc-filter` until the lean feature profile is
   qualified; record the source commit and image
   digest before deployment.
6. Do not deploy an upstream 0.7.4 bump until the PriceTag embedded configs,
   Vertex paths, metering paths, and provider-specific smoke tests pass.

## 5. Control-plane and ExtProc status

The earlier migration plan described the controller migration as greenfield;
that is stale. Current upstream work has already landed meaningful pieces:

### `opendatahub-io/ai-gateway-controller`

- PR #6 (merged) established the ExternalModel routing port, resolver, overlay
  envelope, publisher, and ADR.
- PR #13 (merged) routes ExternalModels through tenant Praxis.
- PR #31 (merged) enables body-based routing.
- PR #52 (merged) routes ExternalModels through tenant ExtProc.
- PR #66 (merged) streams ExternalModel requests for provider selection.
- PR #91 (merged) makes Praxis the default payload-processing dataplane.
- PR #103/#104/#105 (merged) added gateway/provider namespace, stable-ID, and
  MaaSTenantConfig alignment.
- PR #109 remains open for equal-weight provider distribution.
- PR #126 remains open for explicit gateway references and tenant gating.
- PR #128 remains open for the standalone ExternalProvider reconciler.
- PR #129 remains open for provider-qualified Azure credential injection.

Current controller gaps are therefore integration and hardening gaps: weighted
provider semantics, explicit gateway binding, standalone provider lifecycle,
credential strategy alignment, and final tenant/migration cleanup—not the
existence of a controller port.

### `opendatahub-io/praxis-extproc`

- PR #82 (merged) enables body-based routing.
- PR #99/#100/#101/#102/#107/#109 (merged) cover shutdown, cross-phase state,
  route-cache handling, terminal/trailer behavior, and protocol lifecycle.
- PR #123 (merged) documents external-model weight mapping.
- PR #124 (merged) runs the EPP after payload processing.
- PR #128 (merged) documents NeMo guardrail timeout drift.
- PR #136 (merged) propagates `allow_private_upstreams`.
- PR #143 remains open for full-duplex bodies closed by trailers.
- PR #147 remains open for the FIPS/cert-auth Praxis AI 0.6.0 bump.
- PR #129 remains open for buffered agentic Responses support.

The remaining ExtProc risks are lifecycle/body-boundary correctness, FIPS and
dependency alignment, agentic Responses buffering, and production qualification;
the old processing-profile PR #25 is closed, so it should not be listed as an
active implementation PR without a replacement design.

## 6. EnMaaS gaps that remain actionable

### Must retain or revalidate in ET

- API-key validation, tenant identity, and model catalog. (Model access is no longer a dedicated filter: per-model authorization is owned by the metering model-policy preflight with a fail-closed policy decision.)
- PriceTag model-policy preflight and file-backed internal metering auth.
- Qwen-only content/reasoning compatibility behavior is retained temporarily for old running configs and explicitly scheduled for the post-migration cleanup PR.
- Hot-reloaded intelligent-route overlays and provider-specific target-model rewrites.
- Vertex Anthropic rawPredict translation and public-model preservation.
- Mounted GCP key-file token minting plus cluster-scoped credential injection.
- StreamBuffer token-count correction and the EnMaaS 1 MiB scratch setting.

### Integration/operational gates

- Rebase ET from Praxis core/AI 0.7.4, resolving filter registry and API drift.
- Re-run all EnMaaS provider smoke tests: OpenAI, Anthropic, Vertex Anthropic,
  Vertex Gemini, GLM, streaming usage, and failed-request metering.
- Reconfirm filter order: auth → access policy → model/header routing → metering
  → translation/credential injection → token extraction, with no client
  credential leaking to external providers.
- Validate overlay updates, tenant-local routing, provider weights, and status
  ownership against the current controller implementation.
- Prove credential startup seeding, rotation, and deletion behavior without a
  Praxis restart.
- Resolve the FIPS dependency policy separately from the functional ET filter
  map; PR #10 did not introduce a runtime FIPS solution.

## 7. Superseded references

The following statements from the September inventory are no longer reliable:

- “The ExternalModel reconciler is greenfield.” Controller PRs #6, #13, #31,
  #52, #66, and #91 changed that.
- “PR #1356/#1358/#1360/#1372 are open.” They were closed without merging on
  2026-10-10; the ET capabilities remain downstream until re-proposed upstream.
- “Processing profiles are open in praxis-extproc PR #25.” That PR is closed;
  use a new issue/PR if bounded profiles are still required.
- “The ET fork is aligned with upstream.” PR #9 aligned the reviewed snapshot,
  but current divergence is 46 ET-only versus 109 upstream-only commits.

*This file is the downstream/plugin parity source of truth. Product governance
gaps such as SSO, quotas, approvals, and notifications belong in
`redhat-et/pricetag/docs/design/enmaas-capability-gaps.md`, not here.*
