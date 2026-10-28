# 0002. Protocol set and release-gate clients

- Status: Accepted, 2026-10-22

## Context

A mail and groupware server is only useful if the clients people already use work with
it. Supporting a protocol "in principle" is not enough: interoperability defects show up
only when a real client runs against a real server. The protocol set also fixes the shape
of the schema and of the authorisation matrix, so it has to be settled before either is
designed.

## Decision

v1 implements:

- **SMTP**: inbound MX, submission on 587 and 465, an outbound queue, DKIM signing,
  SPF/DKIM/DMARC/ARC checks, MTA-STS, delivery status notifications, PROXY protocol v2.
- **JMAP**: Core, Mail, Submission, VacationResponse, Quotas (RFC 9425), Calendars,
  Contacts (RFC 9610), Sharing and Principals (RFC 9670), Blob (RFC 9404), EventSource.
- **IMAP4rev2** with rev1 compatibility, CONDSTORE/QRESYNC, IDLE, MOVE, ESEARCH, LITERAL+,
  LIST-EXTENDED, LIST-STATUS, SPECIAL-USE, OBJECTID, STATUS=SIZE, QUOTA, ID, UNSELECT,
  NAMESPACE, APPENDLIMIT, and SORT, SORT=DISPLAY, THREAD (ORDEREDSUBJECT, REFERENCES),
  ESORT, CONTEXT=SEARCH/SORT, MULTIAPPEND and CATENATE.
- **CalDAV and CardDAV** with sync-collection, and CalDAV implicit scheduling (RFC 6638).
- **iTIP and iMIP** in and out.
- **Web Push** (RFC 8030, RFC 8291, VAPID) and a Matrix push gateway.
- **Matrix** chat through a homeserver per tenant ([0004](0004-chat-per-tenant-homeserver.md)).

A protocol is done only when its release gates pass:

| Gate | How |
| --- | --- |
| JMAP | OKoala's own conformance suite, and the web client's Playwright suite against the release image (CI) |
| IMAP | Dovecot `imaptest` with all groups, and a test that fails if the number of skipped groups changes; `mbsync` round trip (CI) |
| CalDAV and CardDAV | a subset of Apple's CalDAVTester kept in the repository and growing; `vdirsyncer` round trip (CI) |
| Headless clients | scripted sessions of `imaplib`, `mbsync`, `offlineimap3`, `neomutt`, `aerc`, `vdirsyncer` and Python `caldav` (CI) |
| Two installations | two OKoala installations exchange mail and iMIP over the Internet: SPF, DKIM and DMARC pass, spoofed replies are not applied, roles survive (scheduled CI) |
| Thunderbird | IMAP, submission, CalDAV, CardDAV, autoconfig (manual, recorded) |
| Apple Mail, Calendar, Contacts on macOS and iOS | IMAP with an app password, CalDAV with implicit scheduling, CardDAV, configuration profile (manual, recorded) |
| DAVx5 on Android | CalDAV, CardDAV, sync-collection (manual, recorded) |
| Large remote MTAs | send and receive with Gmail and one other large provider (staging) |
| Chat | the OKoala web client, Element Web and Element Desktop against a tenant's homeserver with SSO; federation between tenants and between installations |

Traffic captured during each manual run is stored under `tests/interop/captures/` and
replayed in CI, so a real-client session is recorded once and checked on every change.

## Consequences

- Every conformance or interop claim in a release is backed by CI or by a recorded run.
- The six IMAP extensions bring their own resource limits (sort keys, context memory,
  multi-append totals), which are part of [0007](0007-limits-as-requirements.md).
- Element X is added as a chat gate once its OIDC-native login is shown to work. Mobile
  Element apps are manual checks only, as native push comes after v1.
- Each manual gate is run again for every release; Outlook (IMAP only) can be added later.

## Alternatives considered

- **IMAP without the sort, thread and multi-append extensions.** Rejected: desktop
  clients then sort and thread on the client, and imaptest skips more groups.
- **Defer CalDAV implicit scheduling.** Rejected: Apple Calendar and Thunderbird send
  invitations through the CalDAV server and rely on it.
- **Only manual client gates.** Rejected: the headless clients are cheap and reproducible,
  and catch most defects before a manual run.
- **Later in v1.x**: IMAP NOTIFY, URLAUTH, METADATA, OAUTHBEARER; JMAP WebSocket and Tasks.
