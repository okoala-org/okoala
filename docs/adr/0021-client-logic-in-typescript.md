# 0021. Client logic stays in TypeScript

- Status: Accepted, 2026-10-22

## Context

The web client contains real logic, not only views: a JMAP client, an IndexedDB sync
engine, authentication, Web Push in a service worker, recurrence editing, and Matrix chat
through matrix-js-sdk with its Rust crypto compiled to WebAssembly. A Rust core compiled to
WASM could share code with the server, and later with native apps. It also brings a larger
bundle, IndexedDB access through bindings, a harder service-worker story, and a second
language in every client change.

## Decision

- Client logic (JMAP client, sync engine, authentication, push, chat) stays in TypeScript.
  There is no Rust/WASM client core in v1.
- Wire types are shared by generation: `client/bindings` holds TypeScript types generated
  from the Rust wire types by ts-rs, checked in and checked in CI.
- Recurrence semantics are kept equal by shared test vectors
  ([0018](0018-time-library-and-wasm.md)).
- The question is revisited only (a) for the recurrence engine alone, if the shared
  vectors show divergence, or (b) when native apps are scoped, after a time-boxed
  evaluation of bundle size, IndexedDB from Rust, the service worker and matrix-rust-sdk in
  the browser.

## Consequences

- Client contributors work in one language and one toolchain.
- The wire contract is still single-sourced in Rust.
- Two implementations of override semantics exist, which the vectors and server-side
  validation guard.
- The only WASM in the client is matrix-js-sdk's crypto, loaded lazily; the
  Content-Security-Policy allows `'wasm-unsafe-eval'` for it.

## Alternatives considered

- **A shared Rust client core in WASM.** Rejected for v1, for the costs above.
- **Hand-written TypeScript wire types.** Rejected: they drift from the server.
