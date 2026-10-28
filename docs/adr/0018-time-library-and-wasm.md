# 0018. Time library, and no WASM bundle of our own

- Status: Accepted, 2026-10-22

## Context

Calendar code does a lot of date arithmetic on user input: alert offsets, durations,
recurrence intervals, time zones. `chrono` is mature and widely used, but some of its
constructors panic on out-of-range values (for example `TimeDelta::days` with a large
value), which turns attacker input into a crash. The server and the web client both
implement recurrence and JSCalendar override semantics; if they disagree, a user sees one
thing and the server stores another. One way to keep them equal is to compile the Rust
engine to WebAssembly for the client; another is to test both against the same vectors.

## Decision

- **Keep `chrono`**, behind a checked-arithmetic wrapper in `okoala-core::time`. Code that
  handles user input uses only the wrapper; panicking constructors are not called on user
  values.
- **No WebAssembly bundle of our own in v1.** The client implements recurrence editing in
  TypeScript ([0021](0021-client-logic-in-typescript.md)).
- **Shared test vectors**: `okoala-model` exports JSCalendar override and recurrence cases
  as `crates/model/vectors/*.json`. The server's tests and the client's Vitest suite both
  run them.
- The server re-validates every patch the client sends.
- The L0 model crates keep compiling to `wasm32`, so a WASM module stays possible later.

## Consequences

- Panics from date arithmetic on user input are designed out rather than caught.
- Two implementations of override semantics exist, kept equal by the vectors and by
  server-side validation.
- If the vectors show divergence that cannot be fixed in TypeScript, shipping the Rust
  recurrence engine alone as WASM is the fallback.

## Alternatives considered

- **Another time library.** Not chosen: the recurrence engine OKoala uses is built on
  `chrono`, and the wrapper addresses the panics.
- **A shared Rust core compiled to WASM for the client.** Rejected for v1: larger
  bundles, a harder build, and the client's other logic (sync, storage, service worker)
  is browser code anyway.
