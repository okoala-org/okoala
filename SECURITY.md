# Security policy

## Reporting a vulnerability

Please report vulnerabilities privately, through GitHub's private vulnerability
reporting:

1. Go to the repository's **Security** tab:
   <https://github.com/okoala-org/okoala/security>.
2. Choose **Report a vulnerability** and fill in the form.

Do not open a public issue, pull request or discussion for a vulnerability.

A useful report says:

- what is affected (file, crate, endpoint, protocol command, chart template);
- how to reproduce it, ideally with a minimal request or test;
- what an attacker gains, and which attacker you assume (see the actors in the
  [threat model](docs/design/threat-model.md));
- the commit you tested.

Reports are read and answered by IASoft engineers. We confirm the issue with you, agree on
a fix and a disclosure date, and publish a GitHub security advisory when the fix is
released. Everyone who reports a valid issue is credited in the advisory, unless they ask
not to be.

## Supported versions

None yet. OKoala is pre-release and not ready for testing or production; there are no
releases to patch. Fixes go to `main`. Once releases exist, this section will list which
of them receive security fixes.

## Scope

In scope: everything in this repository.

- The `okoala` server and its crates, including protocol parsers (SMTP, IMAP, MIME,
  iCalendar, vCard, DAV, JMAP), authentication, authorisation, tenant isolation and secret
  handling.
- The web client.
- The `okoalactl` installer and the Helm chart, including RBAC, NetworkPolicies and
  default configuration.
- Design flaws: a way around a mitigation described in the
  [threat model](docs/design/threat-model.md) or an [invariant](docs/design/invariants.md)
  that does not hold.

Particularly valuable: anything that lets one tenant or user reach another's data or
capacity, account takeover, bypasses of `Authorized<T>` or row-level security, credential
leaks, and escalation from a serving pod.

Out of scope:

- Vulnerabilities in third-party components themselves (for example Ory Kratos and Hydra,
  Tuwunel, k3s, PostgreSQL); please report those upstream. How OKoala configures or uses
  them is in scope.
- Risks the threat model already lists as accepted, unless you show they are worse than
  described.
- Denial of service by traffic volume alone, without a disproportionate cost to OKoala.
- Social engineering, and attacks on infrastructure that OKoala does not ship.
- Installations run by third parties: report to their operator.

## Testing

Test against an installation you run yourself. Do not test against systems you do not own,
and do not access, change or delete other people's data.
