<div align="center">
  <img src="https://cdn.iasoft.dev/okoala/logo.png" alt="OKoala Logo" width="190" style="border-radius: 1.15rem;"/>
  <br/>

  <img src="https://img.shields.io/badge/Email-darkred.svg" alt=""/>
  <img src="https://img.shields.io/badge/Chat-darkgreen.svg" alt=""/>
  <img src="https://img.shields.io/badge/Calendar-darkblue.svg" alt=""/>

  <p style="padding-top: 0.2rem;">
    <b>OKoala: Open Source Collaboration Platform</b>
  </p>
</div>

## Overview

OKoala is a self-hosted, multi-tenant server for email, calendars, contacts and chat, with its own web client. It speaks
open standards, so the mail and calendar apps people already use keep working:

- **Email**: SMTP for receiving and sending (with DKIM, SPF and DMARC), JMAP for Mail, and IMAP4rev2.
- **Calendars and contacts**: JMAP for Calendars and Contacts, CalDAV and CardDAV, and meeting invitations across
  servers (iTIP and iMIP).
- **Chat**: Matrix, with a separate homeserver for each tenant, single sign-on and federation.
- **Web client**: mail, calendars, contacts and chat in the browser, with Web Push notifications.

Each tenant's data is kept apart from the others', and so is its share of the server's capacity: one tenant cannot
slow down another's mail or calendars. OKoala stores its data in PostgreSQL and S3-compatible object storage. It is
deployed with a Helm chart on Kubernetes; a single server runs the same chart on a one-machine k3s cluster, set up by
the `okoalactl` installer.

**Status**: Early Development. **NOT READY** for Testing or Production.

**Design**: [architecture](docs/design/architecture.md), [invariants](docs/design/invariants.md), [threat model](docs/design/threat-model.md), [decision records](docs/adr/README.md), [review checklist](docs/reviews/CHECKLIST.md) and [security policy](SECURITY.md).

## License

Copyright © 2026 IASoft (PVT) LTD.

Licensed under the [AGPL, version 3.0](https://www.gnu.org/licenses/agpl-3.0.en.html). See [LICENSE](LICENSE).

## Use of AI in this project

OKoala is built by the IASoft engineering team, who utilizes Claude (by Anthropic) to speed up the development. 

The engineering team designs every component and writes the security core by hand: authorisation, tenant isolation and
secret handling (listed in [TRUST_LEVELS.toml](TRUST_LEVELS.toml)).

Claude writes most of the other code, and the team reviews every change before it is published here. Commits written by Claude can be isolated via `Co-Authored-By` trailer.
