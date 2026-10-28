# Architecture decision records

Each record states one decision: its context, the decision, its consequences and the
alternatives that were considered. Records are not edited after acceptance except to
correct errors or to note an amendment; a changed decision gets a new record that
supersedes the old one.

The design they add up to is described in [docs/design](../design/architecture.md).

| # | Decision | Status |
| --- | --- | --- |
| [0001](0001-postgresql-only.md) | PostgreSQL is the only database | Accepted, 2026-10-21 |
| [0002](0002-protocol-set-and-release-gates.md) | Protocol set and release-gate clients | Accepted, 2026-10-22 |
| [0003](0003-identity-ory-identity-first.md) | Identity: Ory Kratos and Hydra, identity-first | Accepted, 2026-10-22 |
| [0004](0004-chat-per-tenant-homeserver.md) | Chat: one Matrix homeserver per tenant (with chat domains, signing keys and backups) | Accepted, 2026-10-22 |
| [0005](0005-one-monorepo.md) | One monorepo | Accepted, 2026-10-22 |
| [0006](0006-sharing-semantics.md) | Sharing semantics | Accepted, 2026-10-22 |
| [0007](0007-limits-as-requirements.md) | Limits are product requirements | Accepted, 2026-10-22 |
| [0008](0008-deployment-helm-and-okoalactl.md) | Deployment: Helm only, with the `okoalactl` installer | Accepted, 2026-10-22 |
| [0009](0009-web-client-tested-against-real-server.md) | Web client only, tested against the real server | Accepted, 2026-10-22 |
| [0010](0010-calendar-data-model.md) | Calendar data model: JSCalendar-bis and materialised occurrences | Accepted, 2026-10-22 |
| [0011](0011-licensing.md) | Licensing of OKoala and its dependencies | Accepted, 2026-10-22 |
| [0012](0012-infrastructure.md) | Build, CI and staging infrastructure | Accepted, 2026-10-22 |
| [0013](0013-no-database-migration-protocol-import.md) | No database-level migration; protocol-level import | Accepted, 2026-10-22 |
| [0014](0014-concurrency-and-state-model.md) | Concurrency and state model | Accepted, 2026-10-22 |
| [0015](0015-change-log-retention.md) | Change-log retention | Accepted, 2026-10-22 |
| [0016](0016-search-engine.md) | Full-text search with tantivy behind an indexer role | Accepted, 2026-10-22 |
| [0017](0017-migration-numbering.md) | Migration numbering | Accepted, 2026-10-22 |
| [0018](0018-time-library-and-wasm.md) | Time library, and no WASM bundle of our own | Accepted, 2026-10-22 |
| [0019](0019-background-work.md) | Background work: one job framework with per-tenant fairness | Accepted, 2026-10-22 |
| [0020](0020-internal-api.md) | Internal API: one listener, bearer secret, NetworkPolicy | Accepted, 2026-10-22 |
| [0021](0021-client-logic-in-typescript.md) | Client logic stays in TypeScript | Accepted, 2026-10-22 |
| [0022](0022-trust-levels.md) | Trust levels: who writes each path | Accepted, 2026-10-22; amended 2026-10-28 |

## Adding a record

- Copy the structure of an existing record: Status, Context, Decision, Consequences,
  Alternatives considered.
- Take the next free number. Keep the title short and in sentence case.
- Changing a default limit ([0007](0007-limits-as-requirements.md)), patching an MPL-2.0
  dependency ([0011](0011-licensing.md)) or adding a field to the process `Context`
  ([architecture](../design/architecture.md#process-context)) needs a record.
