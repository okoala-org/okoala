# 0003. Identity: Ory Kratos and Hydra, identity-first

- Status: Accepted, 2026-10-22

## Context

OKoala needs identities, passwords, multi-factor authentication, recovery, sessions and
OAuth 2.0/OIDC tokens for the web client, JMAP and each tenant's chat homeserver. Writing
an identity provider is a large security-critical project. Ory Kratos (identities) and Ory
Hydra (OAuth 2.0 and OIDC) are maintained, Apache-2.0 and widely deployed.

Most account-takeover bugs in multi-tenant systems are not in the identity provider but in
the glue: which tenant an identity belongs to, who may set that, and whether two records
are linked because they share an email address. Desktop mail and calendar clients mostly
cannot do OAuth against a self-hosted provider, so they need another credential.

## Decision

- **Keep Ory Kratos and Hydra.** Kratos holds identities, credentials (password, TOTP,
  WebAuthn), recovery and sessions. Hydra issues tokens. OKoala holds tenants, domains,
  accounts, addresses, groups, roles, quotas, app passwords and suspension.
- **Identity first, no self-registration in v1.** Platform operators create tenants.
  Tenant administrators or operators create accounts through the admin API, which creates
  the account and the Kratos identity in one idempotent job. Users receive an invitation
  (a recovery link) to set their password and MFA.
- **The tenant binding lives in Kratos `metadata_admin`**, which users cannot edit, never
  in user-editable traits.
- **Nothing is linked by email address**, ever. Rebinding an identity is an operator action.
- **The token verifier** requires `aud = okoala`, the `okoala` scope, a `tenant` claim and
  a known, enabled account for `sub`.
- **Logout** revokes the Kratos session and the Hydra tokens, refresh tokens included.
- **App passwords** for IMAP, SMTP submission and DAV Basic, each limited to protocol
  scopes (`imap`, `smtp`, `dav`), with optional expiry, shown once and stored only as
  password hashes.
- **Login, invitation, recovery, settings and consent pages** are part of the web client
  bundle, served by OKoala and using Kratos's browser flows.
- **Development tokens** exist only in a build with the cargo feature `dev-auth`. CI will check
  that the release binary does not contain it.
- **OAUTHBEARER/XOAUTH2** for IMAP and SMTP come after v1, behind a cargo feature that
  release builds leave off and CI will check like `dev-auth`.

## Consequences

- An identity can never move between tenants by its own action; the tenant claim in every
  token comes from data only OKoala and operators write.
- Account creation, disabling and deletion are jobs with retries. An hourly reconciliation
  compares Kratos and OKoala and reports drift; it never creates or links by email, and it
  disables at most a configured number of accounts per run.
- Disabling an account revokes sessions and tokens, suspends app passwords, ends IMAP and
  EventSource sessions on every replica and deactivates the user's chat account.
- The chat sign-in path adds a tenant check in OKoala's consent handler
  ([0004](0004-chat-per-tenant-homeserver.md), invariant I15).
- Self-registration, which needs a verified external address and a tenant policy, is a
  later feature.

## Alternatives considered

- **An embedded OIDC provider in OKoala.** Rejected for v1: it doubles the identity
  surface that has to be written and reviewed.
- **Self-registration into a tenant by email domain.** Rejected: an unverified address
  would bind an identity to any tenant.
- **OAUTHBEARER in v1.** Deferred: there is no evidence yet that mainstream clients use it
  against a self-hosted provider; app passwords cover them.
