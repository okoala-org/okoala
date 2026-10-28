# 0016. Full-text search with tantivy behind an indexer role

- Status: Accepted, 2026-10-22

## Context

JMAP `Email/query` text filters, `SearchSnippet/get` and IMAP `SEARCH BODY/TEXT` need
full-text search over mail. Two kinds of engine fit: PostgreSQL full-text search, or a
separate index such as tantivy (MIT). RFC 9051 also requires IMAP `SEARCH BODY` and
`TEXT` to match substrings, which a word index cannot do.

## Decision

- **tantivy, one index per tenant**, on the volume of an `indexer` role (a StatefulSet),
  driven by the `Email` rows of the change log. `search_state(tenant, account,
  indexed_seq)` records progress.
- **Read your writes**: query results combine index hits with a SQL substring match over
  messages above `indexed_seq`.
- **Queries go through `/internal/v1/search`** on the internal listener
  ([0020](0020-internal-api.md)), with a typed request and response shared by JMAP and,
  when enabled, IMAP.
- **IMAP SEARCH scans by default**: `BODY` and `TEXT` are evaluated by a budgeted scan
  with substring semantics in the CPU pool. An installation may opt into the index for
  IMAP (`imap.search = "index"`), accepting word and prefix semantics.
- Text extraction runs in the CPU pool with panic isolation. A transient blob error is
  retried through the job framework, never recorded as an empty document.
- An optional warm standby indexer builds its own index from the change log and takes
  over through leadership. During failover, search falls back to SQL.
- Indexes are not backed up; they are rebuilt, and the restore drill measures how long
  that takes.

## Consequences

- Body text stays out of PostgreSQL, so the database and its backups stay smaller and
  delivery writes do not compete with index maintenance.
- Search is partitioned per tenant, which keeps capacity isolation and lets a tenant's
  index be dropped and rebuilt alone.
- The indexer's single volume is a failover gap, covered by the SQL fallback and the
  optional standby.

## Alternatives considered

- **PostgreSQL full-text search.** Rejected: storing body text roughly doubles database
  size and backup volume; GIN maintenance competes with delivery on the primary; prefix
  matching across many languages is weaker.
- **An external search service.** Rejected: another stateful service to deploy, secure
  and back up.
