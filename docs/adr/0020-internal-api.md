# 0020. Internal API: one listener, bearer secret, NetworkPolicy

- Status: Accepted, 2026-10-22

## Context

OKoala processes talk to each other for a few things: search queries to the indexer,
cache invalidation hooks, and identity webhooks from Kratos. They also expose probes and
metrics to Kubernetes and Prometheus. Relying on "only reachable from loopback" breaks as
soon as roles run in separate pods, and a sidecar or a misrouted Ingress can expose a port
that was assumed private.

## Decision

- **One internal listener per process**, on port 9090.
- Only `/healthz`, `/readyz` and `/metrics` are unauthenticated.
- **Every `/internal/v1/*` route requires `Authorization: Bearer`** with the
  installation's internal secret, held as a `Secret<T>` and compared with its
  constant-time comparison (constant-time for inputs of equal length; the length is not
  hidden, which a fixed-length random secret makes harmless; an empty stored secret never
  matches).
- Kratos webhooks go to `/internal/v1/identity-hook` with their own secret.
- A **NetworkPolicy** restricts the listener to the `okoala` namespace. The chat namespace
  cannot reach it.
- No route trusts the source address; there are no loopback assumptions.
- Secrets used here are `Secret<T>` values and are rotated with a dual-secret window: the
  verifier accepts the old and the new secret until every replica uses the new one.
- Mutual TLS between processes may be added later.

## Consequences

- A pod that reaches the port still needs the secret; a stolen secret still needs a path
  through the NetworkPolicy.
- Metrics carry no tenant content or credentials, since they are readable inside the
  namespace without the secret.
- Rotation of the internal secrets is a runbook step without downtime.

## Alternatives considered

- **Loopback-only binding.** Rejected: roles run in separate pods.
- **Mutual TLS in v1.** Deferred: it needs certificate issuance per pod and rotation; the
  bearer secret plus NetworkPolicy covers the threat for now.
- **Exposing internal routes on the public listener.** Rejected: one Ingress mistake would
  publish them.
