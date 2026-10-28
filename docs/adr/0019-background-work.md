# 0019. Background work: one job framework with per-tenant fairness

- Status: Accepted, 2026-10-22

## Context

Much of OKoala's work happens after a request returns: sending and applying iTIP messages,
firing alarms, delivering Web Push, extending occurrence horizons, provisioning
identities, deleting accounts and tenants, imports, DSNs, and chat maintenance. Every
replica may run this work. If each feature has its own queue, each needs its own claiming,
retries, timeouts and fairness, and one of them will get it wrong: a slow push endpoint
blocks every other tenant's notifications, or a task that panics stops alarms for
everyone.

## Decision

- **One `jobs` table** in PostgreSQL: `(tenant_id, job_id, kind, account_id, key, run_at,
  attempts, lease_until, lease_token, last_error, payload)`, with a unique `(kind, key)`
  for idempotent enqueue.
- **Claiming** uses `FOR UPDATE SKIP LOCKED`, at most N rows per tenant per pass,
  round-robin over tenants by last service time. Workers renew leases while working.
- **Handlers** run with a timeout, under panic isolation, with exponential backoff, and are
  quarantined after K attempts. Quarantined jobs are visible in the admin API and as a
  metric.
- **Enqueue wakes workers**: a NOTIFY after commit lets any replica pick a job up at once;
  polling remains the fallback.
- **Side effects follow the claim**: logs, metrics, notifications and audit rows about a
  job's outcome are emitted only by the worker whose lease token was accepted on
  completion.
- **Singleton tasks** (maintenance, domain verification, identity reconciliation, the
  occurrence-horizon scheduler, the indexer) run under leadership from
  `pg_try_advisory_lock` on a dedicated connection per process, checked every 5 seconds.
  The provisioner uses a Kubernetes Lease instead, because its work is in the Kubernetes
  API.
- **The outbound mail queue** keeps its own tables (per-recipient rows grouped by
  destination domain) but uses the same claim, lease and fairness library.

## Consequences

- Fairness, timeouts and isolation are implemented once (invariants I12, I13).
- Every job handler must be idempotent: a job may run again after a lease expires.
- A multi-process test asserts that each outcome metric moves once with several replicas.
- Job kinds in v1 include `itip.send`, `itip.apply`, `alarm.fire`, `push.deliver`,
  `occurrences.extend`, `identity.provision`, `identity.disable`, `account.delete`,
  `tenant.delete`, `import.*`, `dsn.send`, `chat.account.deactivate`, `chat.groups.sync`
  and `chat.backup`.

## Alternatives considered

- **A queue per feature.** Rejected: fairness and isolation re-implemented, and missed,
  per feature.
- **An external message broker.** Rejected: another stateful service, and jobs would no
  longer be enqueued in the same transaction as the data that caused them.
- **A dedicated singleton process per background duty.** Rejected: more Deployments, and
  each one a single point of failure.
