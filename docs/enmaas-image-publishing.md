# EnMaaS image publishing

This fork publishes the EnMaaS deployment image separately from the upstream
project's release images. The upstream GHCR release process remains documented
in [the release process](release.md); this document covers only the fork-side
Quay image.

The image is built by `.github/workflows/publish-quay.yaml` on every push to
`main`, with a manual dispatch path for building a selected branch commit,
and pushed to:

```text
quay.io/modden/praxis-ai
```

This is an **interim personal namespace** owned by `@mrodden`. It remains in
place until the planned build and deployment migration. Every build is also
mirrored to the org-owned repository:

```text
quay.io/redhat-et/praxis-ai
```

The mirror is a registry-side `skopeo` copy of the built image at its digest.
It preserves the pushed image index and its attestation manifests
byte-for-byte (a daemon pull/tag/push would flatten the index to a
single-platform manifest at a different digest), and the publish job fails
unless the mirrored digest is identical to the built digest. The run summary
records both repository references. Moving the primary namespace later
requires changing the workflow `IMAGE` value, updating the GitHub Actions
secrets, and updating the cluster pull secret.

## Build configuration

The workflow must keep:

```text
PRAXIS_AI_FEATURES=full,openai-all,store-postgres,gcp-adc-filter,vertex-anthropic-filter,api-key-auth-filter
```

This preserves the fork's required image capabilities after the upstream
0.10 update. Upstream `full` excludes the opt-in OpenAI groups and stores, so
those features must be explicit. The image also includes the automatically
registered `enmaas-policy-filter` workspace crate. The build uses GitHub Actions
cache to avoid rebuilding the Rust dependency graph on every push.

## Triggers and manual rebuilds

- A push to `main` builds the image and updates both the `:main` and
  `:sha-<commit>` tags in each repository. The mutable `:main` tags move only
  while the built commit is still the tip of `main`: publish runs are
  serialized per branch, and a run whose commit has been superseded publishes
  only its `:sha-` tag, so an older build can never move `:main` backward.
- `workflow_dispatch` accepts an optional `commit` input. A manual rebuild
  tags only `:sha-<commit>`; it never moves `:main`.
- Manual dispatch selects a branch in this repository. The optional `commit`
  must be a full lowercase SHA reachable from that branch's tip at dispatch
  time. Tags and unrelated commits are rejected before the registry login.
  Select `shaneutt/update-fork` to build an upstream-update candidate after
  that branch has been reviewed and made available on GitHub.
- The workflow is intended for branches in the canonical repository, where
  the Quay builder secrets are available. Do not expose those credentials to
  arbitrary fork code.

## SBOM artifacts

Every successful build generates two SPDX JSON documents for the source
commit:

- `image.spdx.json`: the packages visible in the exact pushed image digest.
- `source.spdx.json`: the source tree and Rust dependency graph from
  `Cargo.lock`; this is where Rust crates are enumerated even when the
  runtime binary does not carry embedded cargo metadata.

Both files are uploaded together as the Actions artifact
`praxis-ai-sbom-<commit>` with 90-day retention. The job summary records the
image digest for both repositories, the source commit, and the artifact name.

Download the artifact from the workflow run, for example:

```bash
gh run download <run-id> -n praxis-ai-sbom-<commit>
```

The SBOM is inventory evidence, not a signature. Signed provenance and
attestations are still deferred; deploy by digest and retain the SBOM
artifact with the deployment record.

## Ownership

- Primary repository: `quay.io/modden/praxis-ai`
  - Owner: Mathew Odden (`@mrodden`)
- Mirror repository: `quay.io/redhat-et/praxis-ai`
  - Owner: the `redhat-et` organization on Quay (matching the GitHub
    organization); written only by the mirror robot described under
    [Credentials](#credentials)
- Intended long-term target: an org-owned build pipeline

The personal namespace is an interim build path, not a production
supply-chain boundary. Production deployments must pin the digest from an
approved workflow run; the mirror carries that same digest, verified by the
publish job.

## Credentials

Three separate robot accounts are used:

- **Build/push (primary):** a repository-scoped robot with `Write`
  permission on `quay.io/modden/praxis-ai` and no broader access. Its
  credentials are stored only as the `QUAY_USERNAME` and `QUAY_TOKEN` GitHub
  Actions repository secrets and are used only by this workflow.
- **Mirror/push (org):** a robot with `Write` permission on
  `quay.io/redhat-et/praxis-ai`. Its credentials are stored only as the
  `QUAY_ORG_USERNAME` and `QUAY_ORG_TOKEN` repository secrets and are used
  only by this workflow.
- **Pull:** one separate read-only robot with `Read` permission on the
  related EnMaaS image repositories. Its credentials are stored only in the
  cluster pull secrets. It is not used by CI.

The build login uses `docker/login-action`, so the primary builder
credentials are passed as action inputs and never interpolated into a shell
command. The mirror uses `skopeo` with two separate `0600` authfiles, one
per Quay credential, because both repositories share the `quay.io` registry
host and a single docker login cannot hold both. The tokens are read from
stdin and the authfiles are deleted when the mirror step exits. All four
secrets are verified present before the build starts, so a missing robot
credential fails the publish with nothing pushed to either registry, and
the builder login is removed from the runner by an always-run logout at
the end of the job.

## Rotation

Rotate a credential immediately if it is exposed or a maintainer with access
leaves, and otherwise on a regular schedule (at least every 90 days).

1. Create a replacement robot token in Quay with the same scope.
2. Update the single location that holds the old credential:
   - primary builder: the `QUAY_USERNAME` and `QUAY_TOKEN` repository
     secrets;
   - mirror builder: the `QUAY_ORG_USERNAME` and `QUAY_ORG_TOKEN`
     repository secrets;
   - pull: the cluster pull secrets.
3. Run the publish workflow and verify the image pushes, or verify a test pod
   can pull the image.
4. Revoke the old robot token.

## Release and consumption policy

- Every push to `main` produces a development image:
  - `:main` is a mutable convenience tag, updated only by a `main` push whose
    commit is still the branch tip.
  - `:sha-<full 40-character commit SHA>` identifies the build for that commit.
- Manual dispatches produce only the `:sha-` tag for the selected commit.
- Tags are convenience pointers. The image digest is the immutable reference.
- The workflow records `quay.io/modden/praxis-ai@sha256:...` and
  `quay.io/redhat-et/praxis-ai@sha256:...` (the same digest, mirrored and
  verified by the publish job), plus the source commit and the SBOM artifact
  name in the Actions run summary. That digest is the authoritative
  deployment reference.
- Deploy by digest. Never deploy `:main`.
- Promotion is explicit: build the selected branch, validate the digest in staging, then
  update the digest pin in the EnMaaS deployment configuration for staging and
  production.
- Publishing is independent of the upstream Tests workflow, which runs
  separately on `main`. The publish job does not wait for it.
- Version tags and signed provenance attestations are deferred until the
  planned build migration. The OCI source, revision, and version labels
  provide traceability metadata only; they are not a signature.
