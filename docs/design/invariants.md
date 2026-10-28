# Invariants

These are the rules every change to OKoala must keep. Each one has a test or a CI check;
where a rule is enforced by the type system, the type is named. A change that needs to
break an invariant needs a decision record first.

The [architecture](architecture.md) explains the components named here; the
[threat model](threat-model.md) maps threats to these invariants.

| # | Rule, in short | Protects against |
| --- | --- | --- |
| I1 | tenant key first, RLS on every tenant table | cross-tenant reads and writes |
| I2 | account writes only in a locked `AccountWriteTx` | lost updates, wrong states |
| I3 | states read only where the data was read or locked | clients missing changes |
| I4 | every change logged in the same transaction | sync gaps |
| I5 | no I/O or heavy work inside a transaction | lock pile-ups, outages |
| I6 | protocols reach account data only with `Authorized<T>` | authorisation bypass |
| I7 | user-controlled quantities validated at write time | resource exhaustion later |
| I8 | CPU-heavy calls only through `CpuPool` | starved async runtimes |
| I9 | blob fence | serving or losing collected blobs |
| I10 | credentials are `Secret<T>`, never logged | credential leaks |
| I11 | SMTP `250` only after commit | lost mail |
| I12 | background work idempotent and leased | double or lost side effects |
| I13 | fair share of every background queue | one tenant delaying others |
| I14 | serving processes hold no deployment powers | escalation from a compromised pod |
| I15 | chat SSO consent only within the tenant | cross-tenant chat takeover |
| I16 | chat volumes and Secrets kept until an explicit purge | loss of a tenant's Matrix identity |
| I17 | T0 written by hand; no `unsafe`; strict lints in T1 | unreviewed code in the core |

## I1. Tenant key and row-level security

Every tenant table has `tenant_id` first in its primary key and indexes, and a row-level
security policy on `app.tenant_id`.

- **Why**: a missing `WHERE tenant_id = …` in one query must not be enough to read
  another tenant's rows.
- **Enforced by**: a migration lint (`xtask check-schema`); a test that queries every
  table with the wrong tenant set and expects nothing; the transaction entry points set
  `app.tenant_id` with `SET LOCAL`; the server refuses to start with the owner credential,
  since RLS does not apply to the table owner.

## I2. Account writes hold the account lock

Account data is written only inside an `AccountWriteTx` that took the account row lock
first.

- **Why**: states and sequence numbers must be allocated in commit order, and conditional
  writes decided under the lock.
- **Enforced by**: the type system. `AccountWriteTx` can only be obtained from
  `Db::write_account` or `Db::write_accounts`, which lock before running the caller's
  closure ([0014](../adr/0014-concurrency-and-state-model.md)).

## I3. States come from the same snapshot or lock

`ifInState`, `oldState` and `newState` are read only on a locked write transaction, or on
a read snapshot that also served the data.

- **Why**: a state read outside that snapshot can describe different data than the client
  received, and the client then misses or repeats changes.
- **Enforced by**: the type system; state accessors exist only on `AccountWriteTx` and
  `ReadTx`.

## I4. Every change is logged in its transaction

Every change to an object or a container membership appends to `change_log` or
`membership_log` in the same transaction.

- **Why**: the logs drive JMAP `/changes`, IMAP MODSEQ and VANISHED, DAV sync, push and
  indexing; a change missing from them is invisible to every client that syncs.
- **Enforced by**: the store API. Mutations take `&mut AccountWriteTx` and return a
  `Changed` token, and commit requires every token to be recorded.

## I5. Nothing slow inside a transaction

No network I/O, blob I/O or CPU-heavy work runs inside a transaction.

- **Why**: a transaction holds an account lock; a slow S3 call or a large parse inside it
  blocks every other write to that account and ties up a connection.
- **Enforced by**: the write closure makes the transaction's extent explicit; L3 SQL
  modules (`*/src/sql/*.rs`) may not import the blob, HTTP or CPU-pool crates
  (`xtask check-layers`); `statement_timeout` and `idle_in_transaction_session_timeout`;
  a metric and an alert on transactions over 200 ms; an item on the
  [review checklist](../reviews/CHECKLIST.md).

## I6. Protocols need an authorisation proof

Protocol crates reach account data only through `Authorized<T>`.

- **Why**: authorisation is decided once, in one module, for every protocol, instead of
  being re-implemented (and missed) per protocol.
- **Enforced by**: the constructor of `Authorized<T>` is private to `okoala-policy`;
  protocol crates cannot depend on `okoala-db` (`xtask check-layers`); the policy matrix
  test covers every object type and action through every protocol, and CI fails if an
  object type has no matrix entry.

## I7. Quantities are validated when written

Every user-controlled quantity that drives later computation is validated at write time
against `Limits`.

- **Why**: an event accepted today with a pathological recurrence rule costs CPU on every
  later query, alarm and sync. Refusing it at write time is cheap; bounding it later is not.
- **Enforced by**: `okoala-limits::validate::*`, called by the domain write functions; an
  adversarial benchmark per validated quantity with memory and time ceilings
  ([0007](../adr/0007-limits-as-requirements.md)).

## I8. Heavy work goes through the CPU pool

CPU-heavy library calls (recurrence expansion, iCalendar and vCard parsing, MIME parsing
of large parts, tantivy queries) run through `CpuPool::run(budget, f)`.

- **Why**: heavy synchronous work on async worker threads stalls unrelated connections;
  the pool bounds concurrency and charges CPU time to the request's budget.
- **Enforced by**: these APIs are synchronous, and clippy `disallowed-methods` forbids
  calling them from async code outside the pool wrapper.

## I9. Blob fence

A blob row is registered before its bytes become visible, and a blob in state `deleting`
cannot gain a reference.

- **Why**: without the fence, garbage collection can delete bytes that a concurrent write
  is about to reference, or a reader can see bytes the database does not know about.
- **Enforced by**: the register, upload, reference and claim protocol of `okoala-blob`; a
  property test over random interleavings of register, reference, claim, delete and reads
  through process caches after collection.

## I10. Secrets are typed and never logged

Every credential-bearing value is a `Secret<T>`; logs never contain one.

- **Why**: secrets leak through `Debug` output, error messages and serialisation far more
  often than through attacks.
- **Enforced by**: `Secret<T>` has a redacting `Debug`, no `Display` and no `Serialize`;
  a log-scrubbing test runs every integration test with log capture and fails on any known
  test secret in the output.

## I11. SMTP accepts only after commit

SMTP answers 4xx when unsure, and `250` only after the message is committed.

- **Why**: a `250` is a promise to the sender; mail accepted and then lost is not resent.
- **Enforced by**: protocol tests; a crash-safety test that kills the process during
  delivery after DATA and checks that there is neither a `250` nor a row.

## I12. Background work is idempotent and leased

Every background unit of work is idempotent and claims its rows with a lease.

- **Why**: with several replicas, a job may run twice after a lease expires or a process
  dies; it must not send twice or apply twice.
- **Enforced by**: the job framework ([0019](../adr/0019-background-work.md)): unique
  `(kind, key)`, lease tokens checked on completion, side effects emitted only by the
  worker whose lease was accepted; a multi-process test that each outcome is counted once.

## I13. Fair share for every tenant

No tenant can occupy more than its fair share of any background queue.

- **Why**: one tenant's import or invitation storm must not delay another tenant's alarms
  or mail.
- **Enforced by**: per-tenant claim caps and round-robin ordering in the claim library; a
  fairness test in which one tenant has 100,000 due items and another tenant's single item
  is served within one pass.

## I14. No deployment powers in serving processes

Serving processes hold no deployment powers: no Kubernetes API token, no owner database
credential, no access to backups. The `provisioner` pod is the one exception for the
Kubernetes API, limited by RBAC to the kinds it owns, Leases and pods `get/list/delete`,
and never `pods/exec` or `pods/ephemeralcontainers`. The server binary embeds no Docker,
Helm or k3s.

- **Why**: an Internet-facing process will one day have a remote code execution bug; what
  it can then reach must be only its own duty.
- **Enforced by**: a chart test that only the provisioner's ServiceAccount has a mounted
  token and Role bindings, and that its Role lists exactly the expected kinds and verbs;
  `cargo deny` bans container and cluster client crates outside `okoala-provisioner`; the
  refusal to start with the owner credential.

## I15. Chat SSO stays within the tenant

A chat SSO consent is accepted only when the identity's tenant equals the requesting Hydra
client's `metadata.tenant`. Tuwunel's localpart comes only from `preferred_username`,
which OKoala sets to the directory's chat localpart.

- **Why**: Tuwunel cannot restrict login by a claim. Without this check a user of one
  tenant can sign in through another tenant's homeserver and reach an account there with
  the same localpart. Deriving the localpart from an email address would also let two
  addresses with the same local part map to one Matrix account.
- **Enforced by**: consent-handler unit tests; the cross-tenant SSO test in the chat
  end-to-end gate, on every change touching identity or chat; a row in the policy matrix.
  The check runs on every consent request, whether the page is shown, skipped, or
  remembered by Hydra.

## I16. Chat volumes are never deleted implicitly

The provisioner never deletes a tenant's chat volume or Secret except by an explicit purge
with confirmation after the retention period. Volumes are created with
`persistentVolumeClaimRetentionPolicy: Retain`.

- **Why**: the volume holds the tenant's Matrix signing key, which cannot be re-created;
  losing it changes the tenant's identity to every federated server.
- **Enforced by**: provisioner tests on a kind cluster (delete a tenant, then check that
  the volume and Secret remain; a purge without confirmation is refused); chart and
  manifest tests.

## I17. Trust levels hold

T0 code ([0022](../adr/0022-trust-levels.md)) is written by hand: no commit touching a T0
path carries a `Co-Authored-By` trailer naming an AI. T0 and T1 crates use
`#![forbid(unsafe_code)]`, and T1 crates pass the strict clippy set.

- **Why**: the isolation and secret-handling core must be code that a person wrote and
  owns, and no crate needs `unsafe`.
- **Enforced by**: `xtask check-trust` in CI, over the full history of the T0 paths listed
  in [TRUST_LEVELS.toml](../../TRUST_LEVELS.toml); it also checks the crate attributes.
