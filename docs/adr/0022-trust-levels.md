# 0022. Trust levels: who writes each path

- Status: Accepted, 2026-10-22; amended 2026-10-28 (tests of T0 crates are T2)

## Context

OKoala is built by IASoft engineers working with AI agents (see "Use of AI in this
project" in the [README](../../README.md)). Agents write most of the code; engineers
design every component and review every change. A small part of the code is where a single
mistake exposes one tenant's data to another or leaks a credential, and every other crate
goes through it. Trust in that part should not depend on how carefully generated code was
reviewed. It should be written by a person, small enough for one person to own, and
reviewed adversarially.

## Decision

Every path in the repository belongs to one of three levels, listed in
[TRUST_LEVELS.toml](../../TRUST_LEVELS.toml). A file belongs to the level of its longest
matching entry; files that match none are T1.

| Level | Written by | Reviewed by | Contents |
| --- | --- | --- | --- |
| T0 | IASoft engineers, by hand | AI agents briefed as attackers, then the engineers again | `okoala-secret` (`Secret<T>`); the transaction entry points of `okoala-db` (`Db::read`, `write_account`, `write_accounts`, `write_platform`, `ReadTx`, `AccountWriteTx`, setting `app.tenant_id`) and the RLS policy template; `okoala-policy` (`Principal`, `AccessView`, `Policy::authorize`, `Authorized<T>`, and the chat consent tenant decision) |
| T1 | AI agents | IASoft engineers, by risk tier ([review checklist](../reviews/CHECKLIST.md)) | all other production code: crates, the Helm chart, `okoalactl`, the web client |
| T2 | AI agents | IASoft engineers, lightly | tests (including the tests of T0 crates), benchmarks, fuzz targets, `xtask`, CI and build tooling |

Paths are added to T0 as the crates are created; today the list holds `crates/secret/src/`.

- **"By hand"** means an engineer types the code. AI drafts and older code may be read for
  reference, never pasted. Agents may propose changes to T0 as review findings; an
  engineer makes them.
- **T0 holds source files only.** Agents write T0 crates' tests in the crate's `tests/`
  directory, which is T2. T0 crates therefore have no unit tests inside `src/`.
- **Credit follows authorship.** Agent-written commits carry a `Co-Authored-By` trailer;
  commits touching T0 paths never do. Agent-written tests are committed separately from
  T0 source changes.
- **T1 rules**: `#![forbid(unsafe_code)]` (T0 forbids it too); clippy `unwrap_used`,
  `expect_used`, `panic`, `todo`, `unimplemented` and `dbg_macro` denied outside tests;
  `indexing_slicing` and `as_conversions` denied in crates that parse untrusted input.
  Tenant data is reached only through T0's interfaces.
- **Signatures first**: engineers publish T0 signatures before the bodies, so T1 code
  can be built against them.

## Enforcement

- `xtask check-trust` runs in CI. It reads the T0 paths from `TRUST_LEVELS.toml`, fails if
  any commit in the history that touches them carries a `Co-Authored-By` trailer naming an
  AI, and checks the crate attributes (invariant I17).
- The check catches mistakes. The claim that T0 is hand-written rests on the engineers'
  honesty, and is stated publicly in the README.
- Adversarial reviews of T0 are published in sanitized form once every finding is fixed or
  declined: what was reviewed, each finding with its severity and outcome, the fix
  commits, and the claims that held.

## Consequences

- The isolation and secret-handling core is small, stable and owned by people, and its
  review record is public.
- T0 work is on the critical path of everything that touches tenant data, so its
  signatures come first.
- Protocol parsers are not T0. They are large, and fuzzing protects them better than
  authorship; they are reviewed line by line as Tier 1 code instead.
- The levels are unrelated to the crate layers L0 to L5 of the
  [architecture](../design/architecture.md); hence the different names.

## Alternatives considered

- **All code reviewed the same way.** Rejected: one review standard cannot be both
  affordable for all code and strong enough for the isolation core.
- **External audits as the main assurance.** Not chosen: an audit covers one version.
  OKoala relies on evidence anyone can re-check on every commit: public CI, conformance
  results, fuzzing, dependency audits, reproducible and signed builds, and the public
  review records.
- **A per-file "Author" header.** Rejected: files change hands; commit trailers record
  exactly who wrote each change and never go out of date.
