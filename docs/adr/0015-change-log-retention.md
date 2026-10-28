# 0015. Change-log retention

- Status: Accepted, 2026-10-22

## Context

The change log (`change_log`) and the membership log (`membership_log`) are the source of
truth for JMAP `/changes`, IMAP MODSEQ and QRESYNC, DAV sync tokens, EventSource, Web Push
and indexing ([0014](0014-concurrency-and-state-model.md)). They grow with every write.
Kept forever, they dominate the database for busy accounts. Pruned too early, clients that
were offline for a while must resynchronise everything.

## Decision

- A led maintenance task prunes `change_log` and `membership_log` rows **older than 90
  days or beyond 1,000,000 rows per account, whichever is smaller**, and raises the
  account's `min_change_seq`.
- A request for changes from below `min_change_seq` gets the protocol's defined answer:
  JMAP `cannotCalculateChanges`, the DAV `valid-sync-token` precondition failure, and a
  full resynchronisation for IMAP QRESYNC.

## Consequences

- Storage for the logs is bounded per account.
- A client offline for more than 90 days, or an account with more than a million changes
  in that time, does a full resync; every client handles this already, as the protocols
  require it.
- The restore drill checks that each account's `last_seq` covers its change log.

## Alternatives considered

- **Unlimited retention.** Rejected: unbounded growth for busy accounts.
- **A fixed row count only.** Rejected: a quiet account would keep years of history; a
  time bound alone would let a busy account grow without limit.
