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
