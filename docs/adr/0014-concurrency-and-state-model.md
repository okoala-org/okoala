# 0014. Concurrency and state model

- Status: Accepted, 2026-10-22

## Context

JMAP, IMAP and DAV all let clients synchronise incrementally: JMAP through per-type states
and `/changes`, IMAP through MODSEQ and QRESYNC, DAV through sync tokens. Each needs a
state that moves exactly when the data it describes moves, and conditional writes
(`ifInState`, `UNCHANGEDSINCE`, `If-Match`) that are decided under a lock. If a state can
be read outside the transaction that changed it, or if two writers can allocate states
concurrently, clients miss changes or overwrite each other. These mistakes are easy to make
and hard to see in review, so the design makes them impossible to express.

## Decision

`okoala-db` offers four entry points, and only these open transactions on tenant data:

- `Db::read(tenant)`: `REPEATABLE READ, READ ONLY`, with `app.tenant_id` set for RLS.
- `Db::write_account(account, f)`: begins, sets `app.tenant_id` with `SET LOCAL`, takes
  the account row lock (`FOR NO KEY UPDATE`) first, runs the closure, and retries the whole
  closure on serialization failure or deadlock (three attempts with jittered backoff).
- `Db::write_accounts(accounts, f)`: locks several accounts in `(tenant_id, account_id)`
  order, for delivery to several recipients and cross-account copies.
- `Db::write_platform(f)`: platform operations as the `okoala_platform` role.

Rules:

- **Types carry the rules.** An account write is a value of type `AccountWriteTx`, which
  holds the row lock from its first statement. State reads for `ifInState` and `oldState`
  exist only on `AccountWriteTx` and `ReadTx`.
- **One sequence per account, a state per object type.** Each committing write allocates
  the next `seq` from the locked account row. `object_states(tenant, account, type)` is
  updated for each type the transaction changed, so mail delivery does not move contact
  states.
- **IMAP MODSEQ is the change's `seq`.** There is no second counter.
- **Membership tombstones.** `membership_log` records messages, events and cards added to
  or removed from containers. JMAP container changes, DAV sync-collection and IMAP
  VANISHED read it, so every sync protocol is exact.
- **Every mutation is recorded in the same transaction**: mutations take
  `&mut AccountWriteTx` and return a token that commit requires to have been recorded.
- **Transactions are short.** No network I/O, blob I/O or CPU-heavy work inside one.
  Statement timeout 5 s, idle-in-transaction timeout 10 s; a duration histogram per call
  site and an alert above 200 ms p99.
- **Reads**: one `ReadTx` per JMAP method call, so `/get` data and its `state` come from
  one snapshot; IMAP and DAV take one per command. All reads go to the primary.
- **After commit**, the receipt goes to the process's notifier, which sends batched
  `pg_notify` on its own connection. Notifications are hints; the change log is the truth.

## Consequences

- Invariants I2, I3 and I4 are enforced by the type system rather than by review.
- The transaction entry points and the RLS context are T0 code ([0022](0022-trust-levels.md)).
- Write closures must be safe to re-run, which suits the rule against side effects inside
  transactions.
- Per-account locking serialises writes to one account; different accounts write in
  parallel.
- Concurrency tests run against PostgreSQL: concurrent `/set` with the same `ifInState`,
  `oldState` under concurrent delivery, deadlock-free multi-recipient delivery.

## Alternatives considered

- **`SERIALIZABLE` everywhere without an explicit lock.** Rejected: retries under load
  with no ordering guarantee for state allocation.
- **One state for the whole account.** Rejected: every delivery would invalidate every
  client's contact and calendar caches.
- **Deleted flags instead of membership tombstones.** Rejected: a client syncing one
  calendar would not see an event that moved to another.
