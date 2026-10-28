# 0001. PostgreSQL is the only database

- Status: Accepted, 2026-10-21

## Context

OKoala stores tenants, accounts, mail metadata, calendars, contacts, change logs and job
queues in a relational database. A prototype of OKoala supported both SQLite and
PostgreSQL behind one store facade. Keeping two engines meant two schemas, two query sets,
two migration sets and two concurrency models. Behaviour that held on one engine did not
always hold on the other: a transaction that blocked other writers on SQLite did not on
PostgreSQL, so tests passed for the wrong reason. The facade itself was large and every
feature had to touch it.

OKoala's correctness rests on database features: row locks taken at the start of a write,
`FOR UPDATE SKIP LOCKED` for job claims, advisory locks for leadership, `LISTEN/NOTIFY`
for change notification, and row-level security (RLS) for tenant isolation. SQLite has
none of these in the same form.

## Decision

PostgreSQL is the only supported database. Supported versions are PostgreSQL 17 and the
newest major release; CI runs both.

- One set of store crates using `sqlx` compile-time-checked queries against PostgreSQL.
  One offline query cache, checked in CI. One migration set.
- One concurrency model: row locks, `SKIP LOCKED`, advisory locks and `LISTEN/NOTIFY`
  ([0014](0014-concurrency-and-state-model.md), [0019](0019-background-work.md)).
- Every installation, a single machine included, runs PostgreSQL through CloudNativePG
  (a primary with streaming replicas on a cluster, one instance on a single machine), or
  uses an external PostgreSQL such as a managed service.
- For an external server, `okoala admin db init` creates the roles (`okoala_owner`,
  `okoala_app`, `okoala_platform`, `okoala_drill`), the database with `C` collation and
  the RLS settings. With CloudNativePG the Helm chart declares them.
- Backups use CloudNativePG's barman-cloud to an off-host S3 target, with point-in-time
  recovery; an external server is backed up by its provider.
- In v1 all reads go to the primary. Replicas serve high availability and backups.

## Consequences

- One schema to design, review and test. Tenant isolation can rely on RLS everywhere
  ([0014](0014-concurrency-and-state-model.md), invariant I1).
- There is no embedded single-file mode. Evaluating OKoala needs a PostgreSQL server; the
  single-machine installer ([0008](0008-deployment-helm-and-okoalactl.md)) provides one.
- Tests run against a real PostgreSQL, in CI service containers or a local development
  cluster, never against a mock.
- Read scaling through replicas is left for later, because reading from a replica would
  break the guarantee that a JMAP state and its data come from one snapshot.

## Alternatives considered

- **Keep SQLite and PostgreSQL.** Rejected: double the store code and the review load,
  and a second concurrency model in which isolation and locking behave differently.
- **SQLite only.** Rejected: no multi-process writers, no RLS, no `LISTEN/NOTIFY`, so no
  horizontal scaling of the stateless roles.
- **Embed a PostgreSQL binary in the server.** Rejected: upgrades and backups of an
  embedded server are an operator trap, and the server would carry a database it does not
  need while serving mail.
