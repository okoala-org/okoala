# 0005. One monorepo

- Status: Accepted, 2026-10-22

## Context

OKoala has three parts that change together: the shared model crates (JMAP wire types,
JSCalendar, JSContact, recurrence), the server, and the web client. Most changes cross
them: a new JMAP property touches the wire types, the server and the client in one step.
Keeping them in separate repositories means a versioned release of the shared crates for
every contract change, and a window in which the parts disagree.

## Decision

OKoala is one repository, published as `okoala-org/okoala`:

```
crates/        all Rust crates, including the model crates and the okoalactl installer
client/        the web client (Vite, React, TypeScript)
client/bindings/  TypeScript types generated from the Rust wire types (checked in)
tests/         cross-crate integration, conformance, interop and adversarial tests
fuzz/          cargo-fuzz targets
deploy/        the Helm chart, pinned dependency versions, development clusters
docs/          design, decision records, operator documentation
xtask/         repository checks and release tooling
```

- The model crates keep their names and their "no I/O" rule, so they can be published to
  crates.io and npm later if a third party needs them.
- Crates are added when work on them starts; there are no empty placeholder crates.
- Folder names are short (`crates/secret`); package names carry the prefix
  (`okoala-secret`).
- Generated files (TypeScript bindings, the `sqlx` query cache) are checked in and CI
  checks that they are current.

## Consequences

- A contract change lands in one commit with the server, the client and their tests.
- One CI run covers every part, so the merge gate sees the whole system.
- One `Cargo.lock` and one set of pinned dependencies in `[workspace.dependencies]`.
- The repository is larger and CI must partition work to stay fast.

## Alternatives considered

- **Three repositories with semver releases of the model crates.** Rejected: a release
  step on every contract change, and drift between the parts.
- **The server repository as the monorepo, with the model and client repositories as
  read-only mirrors.** Not needed: nothing outside OKoala consumes the crates yet.
