# 0004. Chat: one Matrix homeserver per tenant

- Status: Accepted, 2026-10-22 (including the sub-decisions 4a, 4b and 4c below)

## Context

OKoala offers chat over Matrix. A Matrix `server_name` is permanent: user IDs and room
history are bound to it. One homeserver for the whole installation would tie every
tenant's chat identity to the installation's domain forever, and moving a tenant later
would be impossible. Tenants also need their chat isolated from each other in the same way
as their mail.

Tuwunel (Apache-2.0) is a small Matrix homeserver with RocksDB storage. An evaluation ran
several tenants' instances on one Kubernetes node with SSO through Hydra and federation
between tenants. It showed that the design holds, and that:

- Tuwunel cannot restrict SSO login by a claim, so it cannot refuse a user of another
  tenant by itself. Without a check elsewhere, a user of tenant B could sign in through
  tenant A's homeserver and reach an account of tenant A with the same localpart.
- Tuwunel's signing key exists only in its RocksDB volume, with no import, export or
  rotation. The volume is the tenant's Matrix identity.
- Tuwunel's default IP denylist blocks private ranges, so it reaches other services only
  through public URLs.
- Without a NetworkPolicy any pod in the cluster can reach a homeserver directly.

## Decision

- **One Tuwunel instance per chat-enabled tenant**, on the tenant's primary chat domain,
  fixed at tenant creation. Matrix localparts are unique per tenant across its mail
  domains. Tuwunel is pinned by image digest.
- **The `provisioner` role** of the `okoala` binary, in its own Deployment and
  ServiceAccount, reconciles the directory with Kubernetes by server-side apply: per tenant
  a Secret (created once), a ConfigMap, a Service, a one-replica StatefulSet with a
  retained volume, a Certificate, an Ingress, a NetworkPolicy, and a Hydra OAuth2 client
  `tuwunel-<tenant>` with `metadata.tenant`. It is the only `okoala` pod with a Kubernetes
  token. Its Role is namespaced and limited to those kinds, Leases and pods
  `get/list/delete`; it has no `pods/exec` and no `pods/ephemeralcontainers`.
- **SSO through Hydra with a tenant check in OKoala's consent handler**: a consent is
  accepted only when the identity's tenant equals the client's `metadata.tenant`, the
  account is active and chat is enabled for the tenant. The handler sets
  `preferred_username` to the directory's chat localpart, and Tuwunel derives the localpart
  only from it. Matrix passwords and open registration are off.
- **Per-tenant NetworkPolicies**: ingress only from the Ingress controller; egress only to
  DNS, the Ingress controller and public addresses. Tuwunel reaches Hydra, the push gateway
  and other homeservers through their public URLs, never PostgreSQL, the internal API or
  another tenant's pod. Tuwunel's default IP denylist stays on, against SSRF.
- **An admin bot account per tenant**, registered by the provisioner before any user,
  drives Tuwunel's admin room for backups, deactivation, suspension and room management.
- **OKoala's Matrix push gateway** delivers chat notifications through Web Push; one public
  gateway URL serves every tenant.
- **Federation** is on by default for new tenants, with a per-tenant allow-list and an
  off switch. An application service keeps rooms in sync with directory groups.
- **Upgrades** roll out one tenant at a time after a canary and a fresh backup, stop at the
  first failed health check, roll back the failed tenant and delete a stuck pod on an
  outdated revision.
- **Volumes are never deleted** except by an explicit purge with confirmation after the
  retention period (invariant I16).

### 4a. Chat domain DNS modes

Both modes are supported, the external apex being the default. **Delegated**: the chat
domain points at OKoala and Tuwunel serves the well-known files. **External apex**: the
tenant publishes `/.well-known/matrix/client` and `/server` (or a redirect) on its own
website and a CNAME `matrix.<domain>` to OKoala, which gets a certificate for the
`matrix.` host only. A verifier checks the files and the CNAME before chat is enabled and
re-checks them periodically. The external apex gives user IDs on the tenant's own domain.

### 4b. No signing-key rotation in v1

Tuwunel offers no way to load, export or rotate its signing key. v1 accepts this as a
documented risk. It is mitigated by scheduled, encrypted, off-volume backups whose
decryption key never enters the cluster. A suspected key exposure has no supported remedy
in v1; key import, export and rotation are pursued upstream.

### 4c. Backups through the admin bot

Online backups are triggered by an admin-room command of the per-tenant admin bot, not by
a signal sent from an ephemeral debug container. The bot is needed anyway for
deactivation, suspension and room management, and the ephemeral-container permission
would let the provisioner read every tenant's volume.

Backups are encrypted asymmetrically: the cluster holds only the public key. Restores and
restore drills run from the operator's machine (`okoalactl restore`, `okoalactl
restore-drill`), which supplies the private key.

## Consequences

- No later migration from a shared homeserver is ever needed.
- Each tenant costs a pod and a volume even when idle; installations cap the number of
  chat-enabled tenants ([0007](0007-limits-as-requirements.md)).
- One replica per tenant: an upgrade interrupts that tenant's client API for a few
  seconds.
- Losing a volume without a backup loses the tenant's Matrix identity, history and
  devices; backups are scheduled, alerted on and drilled.
- Unattended in-cluster restore drills are not possible for chat backups; operators run
  them on a schedule, with an alert when one is overdue.
- The consent tenant check is security-critical code (T0, [0022](0022-trust-levels.md))
  with a cross-tenant SSO test in the chat end-to-end gate.

## Alternatives considered

- **One homeserver per installation.** Rejected: the permanent `server_name` would bind
  every tenant to the installation's domain.
- **Chat after v1.** Rejected: a homeserver per tenant can be built from the start, which
  avoids a later move of tenants' chat identities.
- **The provisioner as a separate binary.** Rejected: its powers come from its
  ServiceAccount, not its binary, so separation would not narrow them.
- **A delegated chat subdomain only** (4a). Rejected: user IDs on a subdomain, when the
  tenant can publish two files instead.
- **Backups by signal from an ephemeral container** (4c). Rejected: it needs
  `pods/ephemeralcontainers`, which amounts to reading every tenant's volume.
- **A backup key in a Kubernetes Secret.** Rejected: a compromised cluster or a stolen S3
  credential could then read the backups, which hold signing keys and access tokens.
