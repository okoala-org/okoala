# 0012. Build, CI and staging infrastructure

- Status: Accepted, 2026-10-22

## Context

A test that only passes on someone's machine is not evidence. Every claim OKoala makes
about conformance, interoperability or isolation should be reproducible by CI, or recorded
from a run on a staging installation. Rust release builds are slow on small machines, and a
build machine that is also used for browsing, mail and other tools is an easy way into the
supply chain.

## Decision

- **CI runs on Linux runners**: ephemeral GitHub-hosted runners, larger runners for heavy
  jobs. Builds use a shared compilation cache and partitioned test runs.
- **CI runners and any build host are project-only machines**: they run the project's
  builds and nothing else.
- **Release images are built only by CI**, never on a developer machine or a test server.
- **The public repository runs CI on every published push** and builds releases, so that
  release provenance and signatures point at the public source.
- **Staging** is a multi-machine Kubernetes cluster that always runs the current `main`
  with the full feature set: CloudNativePG with replicas, Ory, two replicas of every
  stateless role, several chat tenants, and a test domain for outbound mail.
- **A single-machine installation** (`okoalactl install --single`) runs alongside staging
  as the second installation for the two-installation interop tests
  ([0002](0002-protocol-set-and-release-gates.md)). It never builds images.
- **An interop lab** with a Mac (Apple clients and the iOS simulator) and an Android
  emulator runs the manual client gates against staging.
- Workflows use the built-in `GITHUB_TOKEN` with least-privilege `permissions:`, OIDC
  federation instead of stored long-lived secrets where the provider supports it, and
  environment-scoped secrets for staging. Third-party test images are pinned by digest and
  mirrored. The Rust toolchain is pinned in `rust-toolchain.toml`.

## Consequences

- Staging runs everything at once, so defects that only appear when all features are on
  are found before release.
- Two installations exchange mail, invitations and federation over the Internet, which a
  single installation cannot test.
- Until staging exists, split-mode evidence comes from multi-process tests in CI and from
  a Kubernetes-in-Docker cluster, which is weaker.

## Alternatives considered

- **macOS developer machines as builders.** Rejected: production is Linux, and developer
  machines are not project-only.
- **Building images on the test servers.** Rejected: slow on small machines, and it puts
  build tooling and credentials on hosts that serve traffic.
