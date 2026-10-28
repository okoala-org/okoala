# 0013. No database-level migration; protocol-level import

- Status: Accepted, 2026-10-22

## Context

People moving to OKoala bring mail, calendars and contacts from other software. A
converter that reads another system's database directly is tied to that system's schema
and version, is used once per move, and needs full access to all data. Standard export
formats already exist for every kind of data OKoala stores.

## Decision

- **No database-level migration** from earlier software, including earlier OKoala
  software.
- Data comes in through **protocol-level import**: mail as mbox (or Maildir) with
  `okoala admin import mbox`, or pulled over IMAP later; calendars as `.ics`; contacts as
  `.vcf`. Imports run through the admin CLI or JMAP, as fair, per-tenant jobs
  (`import.mbox`, `import.ics`, `import.vcf`).
- Domains and accounts are created through the admin API.

## Consequences

- The importers are product features, tested and documented like any other.
- Imported data goes through the same validation and limits as data from clients
  ([0007](0007-limits-as-requirements.md)).
- Server-side state that has no standard format (sessions, push subscriptions, app
  passwords) is not carried over; users sign in and re-register.

## Alternatives considered

- **A schema converter.** Rejected: code tied to one foreign schema, used once, with full
  data access.
