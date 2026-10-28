# Review checklist

Every commit published in this repository has been reviewed by an IASoft engineer.
"Reviewed" means the reviewer went through this checklist, not that they skimmed the diff.
Contributors should check their own changes against it before opening a pull request;
reviewers use the same list.

## How deep a review goes

Review depth depends on risk. A change is reviewed at the tier of the riskiest code it
touches.

| Tier | Code | Review |
| --- | --- | --- |
| 1 | T0 ([trust levels](../adr/0022-trust-levels.md)); authentication, sessions, tenant and permission checks; cryptography and secrets; parsers of untrusted input (IMAP, SMTP, MIME, iCalendar, vCard, DAV XML, JMAP); deployment security (RBAC, NetworkPolicy); migrations that delete data | every line, and the tests that cover it |
| 2 | storage, business logic, background jobs, protocol behaviour | the full diff and its tests |
| 3 | UI, documentation, build tooling, test helpers | the diff, and the behaviour through tests or screenshots |

## Every change

- [ ] I can explain what it does and why it is needed.
- [ ] It does nothing the task did not ask for, and has no dead code.
- [ ] Errors and limits are handled; untrusted input is bounded.
- [ ] Tests cover the change and fail without it.
- [ ] Names, comments and the commit message are clear.
- [ ] New dependencies are justified and pass `cargo deny`.

## Server code

- [ ] **Authorisation**: account data is reached only through `Authorized<T>` from
  `okoala-policy` (I6). New object types or actions add rows to the policy matrix test.
- [ ] **Existence does not leak**: a forbidden object is "not found", identical to an
  unknown one, in every protocol.
- [ ] **Tenant keys**: new tenant tables lead with `tenant_id` and have an RLS policy
  (I1); new account tables lead with `tenant_id, account_id`.
- [ ] **Writes**: account data is written only in an `AccountWriteTx` (I2), and every
  change is recorded in the change log (I4).
- [ ] **Nothing heavy inside a transaction**: no network I/O, blob I/O or CPU-heavy work
  between the transaction's start and its commit (I5).
- [ ] **Limits**: every new user-controlled quantity is validated at write time and has a
  default in `okoala-limits` and an adversarial benchmark (I7). Work per request is
  charged to the `Budget`.
- [ ] **CPU-heavy calls** run through `CpuPool` (I8).
- [ ] **Secrets** are `Secret<T>`, are never logged, formatted or serialised, and are
  compared with the constant-time comparison only when they are high-entropy machine
  secrets; user passwords are stored and checked as password hashes (I10).
- [ ] **Error mapping**: SMTP answers 4xx when unsure and `250` only after commit (I11);
  budget and limit errors map to the protocol's defined error.
- [ ] **Background work** is idempotent, leased and fair per tenant; outcome side effects
  are emitted only by the worker whose lease was accepted (I12, I13).
- [ ] **Multiple replicas**: the change is correct with two or more processes; nothing
  assumes a single process or loopback.
- [ ] **No `unwrap`, `expect`, `panic` or `unsafe`** outside tests (I17).

## Deployment and operations code

- [ ] No new Kubernetes permission for any pod except as recorded in a decision record;
  the provisioner's Role changes only with its chart test (I14).
- [ ] No secret generated at chart render time; no password on a command line.
- [ ] Every Job and CronJob has a deadline.
- [ ] Chat volumes and Secrets are never deleted except by an explicit purge (I16).

## T0 changes

- [ ] The change was written by hand by an IASoft engineer; the commit carries no AI
  co-author trailer (I17). Tests for it are in the crate's `tests/` directory and
  committed separately.
- [ ] It has been reviewed adversarially, and every finding is fixed or declined with a
  reason before the change is published.

## Fixes for review findings and reported vulnerabilities

- [ ] Each finding has a test that fails before the fix.
- [ ] Similar code elsewhere was checked for the same defect.

## Comments and commits

- [ ] Comments explain why (an invariant, an RFC section, a security reason, a trade-off),
  not what the next line does. No narration and no comments addressed to the reviewer.
- [ ] No speculative abstraction, unused options or code kept "for future use".
- [ ] Each commit does one thing, with a subject such as `[AREA]: ...` and a body saying
  what and why.
- [ ] New source files start with the copyright line and the SPDX identifier
  (`AGPL-3.0-only`) and nothing else.
- [ ] Anything the reviewer could not defend in a public issue is rewritten or removed.
