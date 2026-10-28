# 0009. Web client only, tested against the real server

- Status: Accepted, 2026-10-22

## Context

OKoala ships its own client for mail, calendars, contacts and chat. Third-party clients
cover IMAP, SMTP and DAV ([0002](0002-protocol-set-and-release-gates.md)), so OKoala's
own client only needs to exist where people expect a browser. Desktop and mobile shells
add packaging, signing, store review and native push for every platform.

A client tested against a hand-written fake server tests the fake. The fake drifts from
the server, and both sides can pass their tests while disagreeing on the wire.

## Decision

- **v1 has a web client only.** Desktop and mobile shells come after v1, built from the
  finished web client.
- **No fake server.** Unit tests use MSW only for HTTP edge cases (timeouts, 5xx,
  malformed responses). Integration tests run against `okoala serve --test --seed=<scenario>`,
  which boots a real server with seeded data in about a second. End-to-end tests run
  Playwright against the release container image with PostgreSQL and S3.
- **Chat is tested against a real Tuwunel**, not a fake homeserver.
- The server's conformance suite and the decision records are the contract between client
  and server. Wire types come from `client/bindings`, generated from the Rust types and
  checked in CI.
- One integration run uses a server configured with the smallest legal limits, so the
  client is shown to honour every advertised limit.

## Consequences

- A contract change that breaks the client fails CI in the same change.
- Client tests need a built server; CI builds the image once and shares it.
- Native push (APNs, FCM) is not needed in v1; chat and mail notify through Web Push.
- Tests wait on observable state, not fixed sleeps, so they hold on a loaded CI runner.

## Alternatives considered

- **A fake JMAP server for client tests.** Rejected: it drifts from the server and hides
  contract defects.
- **Desktop and mobile apps in v1.** Rejected: packaging and native push for several
  platforms before the web client is finished.
