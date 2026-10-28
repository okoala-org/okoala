# 0007. Limits are product requirements

- Status: Accepted, 2026-10-22

## Context

In a multi-tenant server the most likely attacker is an authenticated user or a hostile
tenant, not an anonymous host. Their easiest attack is resource exhaustion: an event with
a recurrence rule that expands to millions of instances, a deeply nested MIME message, an
IMAP SORT over huge subjects, or a flood of jobs that delays every other tenant's alarms.
A limit that exists only as a hard-coded constant is easily lost in a refactor. A limit
that is not tested may not hold.

## Decision

Default limits are product requirements. Operators can change them per installation and
per tenant. A default changes only through a new decision record, with load-test evidence.

One crate, `okoala-limits`, owns them, with one configuration section `[limits]` and
per-tenant overrides:

- **A request budget** (deadline, response bytes, rows read, CPU time, derived objects) is
  created per request or command, passed to every domain call and charged as work happens.
  Exceeding it is a typed error mapped per protocol.
- **Write-time validation** of every user-controlled quantity that drives later work
  (overrides, alerts, recurrence density, participants, JSON depth, header and MIME
  structure, literal sizes, names), with checked arithmetic only.
- **Limiters** for security counters (authentication failures, sending, external iTIP,
  password resets) shared by every replica in PostgreSQL; request throttles in memory per
  process, divided by the configured replica count.
- **Concurrency caps** per account and per process.
- **Fairness** for background work: claims take at most N rows per tenant per pass,
  round-robin by last service time.

Selected defaults:

| Quantity | Default |
| --- | --- |
| Message size (SMTP, APPEND, upload) | 50 MiB |
| Recipients per message / per account per hour / per tenant per hour | 100 / 500 / 10,000 |
| JMAP `maxSizeRequest` / `maxCallsInRequest` / `maxObjectsInGet` / `maxObjectsInSet` | 10 MB / 32 / 500 / 500 |
| Response bytes per request (JMAP, DAV), streamed | 64 MiB |
| CPU time per request | 2 s |
| Event size / overrides / alerts / participants | 512 KiB / 1,000 / 32 / 1,000 |
| Occurrences materialised per event within the horizon | 5,000 |
| IMAP line / literal / sessions per account | 64 KiB / 50 MiB / 20 |
| IMAP CONTEXT memory per session / per process | 64 MiB / 512 MiB |
| DAV request body / hrefs per multiget | 4 MiB / 1,000 |
| JSON nesting depth | 64 |
| Concurrent JMAP / DAV requests per account | 4 / 4 |
| App passwords per account | 50 |
| Matrix push gateway request body / devices per request | 64 KiB / 32 |

Chat instances have chart defaults for CPU and memory requests and limits, and each
installation caps the number of chat-enabled tenants, so that idle instances cannot
exhaust a cluster.

## Consequences

- Every validated quantity has an adversarial benchmark in `tests/bench` with a memory and
  time ceiling, run nightly (invariant I7).
- CPU-heavy work runs through a bounded pool and is charged to the budget (invariant I8).
- A fairness test checks that one tenant with 100,000 due items does not delay another
  tenant's single item by more than one pass (invariant I13).
- The web client honours every advertised limit, including the smallest legal values.
- Some numbers (push-gateway rate, group-room sizes) are starting values to be confirmed
  by load tests.

## Alternatives considered

- **Limits as implementation details.** Rejected: they would not be tested or documented,
  and operators could not reason about capacity.
- **Only global rate limits.** Rejected: they do not stop one expensive request or one
  tenant's backlog.
