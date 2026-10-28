# 0017. Migration numbering

- Status: Accepted, 2026-10-22

## Context

Schema migrations live in one directory, `crates/db/migrations/`, named `NNNN_name.sql`.
Several changes are often developed in parallel on separate branches. If each branch picks
the next free number itself, two branches pick the same one and the conflict appears only
at merge, or worse, after both have been applied to a test database. Timestamps avoid
collisions but make the order depend on when work started rather than on review.

## Decision

- Migrations use **sequential numbers allocated from a registry file**,
  `crates/db/migrations/REGISTRY.md`, when work on a change begins. The registry records
  the number, the name and the change it belongs to.
- Each change carries at most one migration.
- CI checks that the files and the registry agree, with no gaps or duplicates.
- **Pre-release squash**: until the first staging deployment with real users, the initial
  migrations may be squashed once. After that, migrations are append-only.
- Migrations are expand and contract: a release's migrations work with the previous
  release's binary, and destructive steps wait one release. `okoala serve` refuses a
  schema newer than it knows or older than it needs.
- Migrations run only in the `migrate` Job with the owner credential, and must succeed
  before serving pods start.

## Consequences

- Two branches never claim the same number.
- An abandoned change leaves a gap only if its number is released in the registry.
- Rolling upgrades with mixed versions work, and are tested nightly.

## Alternatives considered

- **Timestamp-named migrations.** Rejected: order by start time, not merge order, and
  harder to review as a sequence.
- **Numbers picked at merge time.** Rejected: renaming on merge breaks references in the
  change's tests and documents.
