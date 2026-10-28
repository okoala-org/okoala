# 0013. No database migration from the prototype; protocol-level import

- Status: Accepted, 2026-10-22

## Context

Before v1, a prototype of OKoala ran with SQLite and PostgreSQL schemas that v1 replaces
([0001](0001-postgresql-only.md), [0014](0014-concurrency-and-state-model.md)). Its
installations hold test data. A converter from the old schemas would be code written once,
used once, and reviewed with the care that any code touching all data needs. Importers for
standard formats are product features anyway: new users move from other servers the same
way.

## Decision

- **No database-level migration** from the prototype.
- Domains and accounts are recreated through the admin API.
- Mail is exported per account as mbox and imported with `okoala admin import mbox`;
  calendars and contacts are exported as `.ics` and `.vcf` and imported the same way, or
  through JMAP. Imports run as fair, per-tenant jobs (`import.mbox`, `import.ics`,
  `import.vcf`).
- **No secret is carried over.** v1 generates all keys afresh: new DKIM keys under a new
  selector, published in DNS before v1 signs with them; new certificates, database and S3
  credentials, Ory secrets and VAPID keys. Test hosts of a prototype are not held to the
  standard v1 requires for key handling, so nothing from them is trusted.

## Consequences

- The importers are tested and documented as features (mbox, Maildir, iCalendar, vCard).
- Moving the prototype's data is also the first rehearsal of the key-rotation runbooks.
- Web Push subscriptions do not survive the move; clients re-register.

## Alternatives considered

- **A schema converter.** Rejected: throwaway code with full data access.
- **Reuse the prototype's DKIM keys so DNS need not change.** Rejected: a new selector is
  cheap, and v1 should start with keys whose whole history is known.
