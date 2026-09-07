# telemaco-js

V8 JavaScript runtime (`deno_core`) and DOM bindings for the
[Telemaco](https://github.com/AlbertoBarrago/telemaco) headless browser.

## Place in the workspace

The scripting layer. `src/runtime.rs` owns the isolate and the per-page state,
`js/bootstrap.js` is the DOM and browser shim executed inside V8, and
`src/ops.rs` is the single bridge between JS and Rust. It sits above
`telemaco-dom` (the tree it exposes to scripts) and `telemaco-net` (the
transport behind `fetch()` and `XMLHttpRequest`), and, with `render`, above
`telemaco-render` for real box geometry. `telemaco-browser` is the layer above
and is what application code should use.

This is an internal layer. If you want to drive a page from Rust, depend on the
[`telemaco`](https://github.com/AlbertoBarrago/telemaco/tree/main/crates/telemaco)
crate instead of embedding this runtime yourself.

## Features

| Feature | Effect |
|---------|--------|
| `stealth` | Enables `telemaco-net/stealth` so scripted `fetch()` and XHR go through the wreq/BoringSSL client and carry the same TLS fingerprint and client hints as stealth navigation. |
| `render` | Enables `telemaco-render` with its `paint` feature: real box geometry for `getBoundingClientRect`, `elementFromPoint` and `offset*`, plus PNG rasterization (`screenshot_png` and friends are re-exported here). Off by default to keep the scraping build lean. |

## Invariants

- **One V8 isolate per process.** Isolate creation is serialized
  (`ISOLATE_CREATE_LOCK` in `runtime.rs`) and callers above must serialize
  execution. This is why the test suite runs under `cargo nextest` (one process
  per test) and not `cargo test`.
- **Ops must be panic-safe.** `op_dom` is wrapped in `catch_unwind` so a DOM-op
  panic returns null instead of aborting inside V8's FFI frame. Unwinding into
  V8 aborts the process, so new ops must not do it. The release profile pins
  `panic = "unwind"` for the same reason.
- **A page must never hang the process.** The termination watchdog
  (`spawn_watchdog` in `runtime.rs`) terminates the isolate from a separate
  thread, because a timeout only fires at await points while synchronous V8 work
  runs unbounded.
- Nothing in this crate is `Send`: the isolate and the DOM live on one thread,
  which is why all async above runs in a tokio `LocalSet`.

Part of [Telemaco](https://github.com/AlbertoBarrago/telemaco).
