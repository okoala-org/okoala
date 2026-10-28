# 0010. Calendar data model: JSCalendar-bis and materialised occurrences

- Status: Accepted, 2026-10-22

## Context

Events reach OKoala as JSCalendar (JMAP) and as iCalendar (CalDAV, iMIP). Recurring events
need range queries, free/busy, alarms and CalDAV time-range reports. Expanding recurrence
rules on every query costs CPU proportional to the rule's density and the queried range,
which an attacker controls. The JMAP Calendars draft is written against JSCalendar-bis,
the revision of RFC 8984, not against RFC 8984 itself.

Converting between iCalendar and JSCalendar loses information in subtle places
(participant roles, `PRODID`, all-day overrides with `RECURRENCE-ID;VALUE=DATE`,
`RANGE=THISANDFUTURE`, local times in DST gaps or overlaps, vCard 3 `TYPE` values).

## Decision

- **Store each event's JSCalendar object whole**, in JSCalendar-bis, moved to before the
  first release.
- **Materialise occurrences** at write time over a rolling horizon of 12 months back and
  18 months ahead, in `event_occurrences`, with the horizon per event in `event_horizons`.
  The expansion is computed from a read snapshot in the CPU pool, then written in the
  event's write transaction, which checks that the event has not changed meanwhile and
  retries if it has.
- **Dense rules**: a rule with more than 5,000 occurrences in the horizon is marked
  `dense` and expanded on demand within the request budget.
- **Horizon extension** runs as fair, per-tenant `occurrences.extend` jobs.
- **Queries** (JMAP time ranges, free/busy, CalDAV `calendar-query`, alarms) read
  `event_occurrences` through an index; outside the horizon they expand on demand with the
  budget.
- **Alarms** are `alarm_triggers` rows derived at write time, including each sharee's own
  alerts; a scheduler enqueues `alarm.fire` jobs for due triggers. No periodic rescans.
- **CalDAV keeps converting.** GET returns iCalendar converted from the stored JSCalendar.
  There is no byte fidelity, so PUT returns no ETag (RFC 4791 §5.3.4); CalDAVTester's
  exact-match scripts are recorded as expected failures.
- **iCalendar and vCard parsing** uses calcard only behind OKoala's own adapter
  (`okoala-ical`), with its own corpus tests for the cases above and VTIMEZONE components
  generated from the tz database.

## Consequences

- Range queries, free/busy and alarms are index lookups with bounded cost.
- Writes of recurring events cost more, and storage grows with the horizon; both are
  bounded by the limits in [0007](0007-limits-as-requirements.md).
- If the JSCalendar-bis or JMAP Calendars drafts change again, the wire mapping changes;
  it is isolated in `okoala-model`.
- CalDAV clients such as vdirsyncer download an item once more after uploading it.
- Storing the client's iCalendar bytes alongside (byte fidelity) may come in v1.x.

## Alternatives considered

- **Expand on every query.** Rejected: unbounded CPU under attacker-chosen rules.
- **Store iCalendar as the source of truth.** Rejected: JMAP is the primary API, and every
  JMAP read would convert.
- **Store both representations in v1.** Deferred: more storage and two representations to
  keep consistent.
- **Stay on RFC 8984.** Rejected: the JMAP Calendars draft uses bis-only properties.
