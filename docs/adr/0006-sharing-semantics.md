# 0006. Sharing semantics

- Status: Accepted, 2026-10-22

## Context

Users share calendars and address books with each other. JMAP Calendars, JMAP Contacts
(RFC 9610) and JMAP Sharing (RFC 9670) define per-collection rights; CalDAV and CardDAV
clients see the same shares through the principal's home. Sharing is where authorisation
is most subtle: rights interact with patches, a sharee may change some properties of a
shared event but not others, and a grantee with the right to share must not escalate its
own rights. What a grantee can be (a user, a group, a user in another tenant) changes the
authorisation matrix and the change fan-out, so it is expensive to change after release.

## Decision

- **Rights are per collection** (calendar, address book), as in JMAP: `mayReadFreeBusy`,
  `mayReadItems`, `mayWriteAll`, `mayWriteOwn`, `mayUpdatePrivate`, `mayRSVP`, `mayShare`,
  `mayDelete`, and the contacts subset.
- **Grantees are users and groups** in v1.
- **A `mayShare` grantee** can grant at most the rights it holds and cannot change its own
  entry.
- **Owner-only operations** are decided by ownership, never by a rights value.
- **Cross-tenant sharing** requires an explicit grant, and both tenants' policies must
  allow it. The default is deny.
- **Unknown and forbidden principals give identical errors**, so sharing cannot be used
  to probe for accounts.
- **Per-user properties of shared events** (alerts, colour, keywords, `useDefaultAlerts`)
  are stored per sharee, never on the owner's copy.
- **Patch-level rules** for `mayRSVP`, `mayUpdatePrivate` and `mayWriteOwn` are
  implemented once, on flat patch maps with the JSCalendar override semantics, and used by
  every protocol.
- **Mail sharing** (shared mailboxes, IMAP ACL) comes after v1. The policy module is
  designed so that it adds an object type, not a new mechanism.

## Consequences

- All of this is decided in `okoala-policy` ([0022](0022-trust-levels.md), invariant I6).
  Writes re-check rights inside the write transaction, so a revocation between read and
  write is honoured.
- The policy matrix test enumerates owner, each single right, group grantee, other tenant
  under each policy, disabled account and suspended tenant against every object type and
  action, through every protocol that exposes the object.
- Group grantees mean a membership change can change effective rights; change
  notifications are filtered per subscriber through its access view.
- Sharees' own alarms fire for them, from their own per-user properties.

## Alternatives considered

- **User grantees only, groups later.** Rejected: adding group grantees after release
  changes the matrix and the fan-out.
- **Per-object rights.** Rejected: neither JMAP nor CalDAV clients present them, and they
  multiply the matrix.
- **Cross-tenant sharing on by default.** Rejected: tenants are isolated unless both opt
  in.
