# 0012. Build, CI and staging infrastructure

- Status: Accepted, 2026-10-22

## Context

A test that only passes on someone's machine is not evidence. Every claim OKoala makes
about conformance, interoperability or isolation should be reproducible by CI, or recorded
from a run on a staging installation. The machines that build releases are part of the
supply chain and must be treated as such.

## Decision

- **CI runs on ephemeral Linux runners** that are used for the project's builds and
  nothing else. Builds use a shared compilation cache and partitioned test runs.
- **Release images are built only by CI**, never on a developer machine or a test server.
- **The public repository runs CI on every published push** and builds releases, so that
  release provenance and signatures point at the public source.
- **Staging always runs the current `main`** with the full feature set: replicated
  PostgreSQL, Ory, two replicas of every stateless role, several chat tenants, and a test
  domain for outbound mail.
- **A second, single-machine installation** (`okoalactl install --single`) is the peer for
  the two-installation interop tests ([0002](0002-protocol-set-and-release-gates.md)). It
  never builds images.
- Workflows use the built-in `GITHUB_TOKEN` with least-privilege `permissions:`, OIDC
  federation instead of stored long-lived secrets where the provider supports it, and
  environment-scoped secrets for staging. Third-party test images are pinned by digest and
  mirrored. The Rust toolchain is pinned in `rust-toolchain.toml`.

## Consequences

- Staging runs everything at once, so defects that only appear when all features are on
  are found before release.
- Two installations exchange mail, invitations and federation over the Internet, which a
  single installation cannot test.
- Hosts that serve traffic carry no build tooling or build credentials.

## Alternatives considered

- **Building images on developer machines or test servers.** Rejected: such builds are
  not reproducible from the public source, and they put build tooling and credentials on
  machines used for other things.
