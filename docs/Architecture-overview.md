Telemaco is a workspace of nine crates.

```
telemaco-cli       CLI entry point. fetch, scrape, mcp, install, update, remote.
telemaco-browser   Page type, navigation, lifecycle events.
telemaco-js        V8 runtime via deno_core. bootstrap.js + Rust ops.
telemaco-dom       DOM tree implementation.
telemaco-net       HTTP client, stealth client, cookie jar, robots cache, tracker blocklist.
telemaco-mcp       Model Context Protocol server.
telemaco-render    CSS cascade, retained layout, text shaping, and CPU paint.
telemaco-remote    Remote protocol and transports (Tailcat, local). No V8.
telemaco           Embeddable Rust library API (Browser, Page, Element, CookieStore).
```

## Request flow

A `browser_navigate` call from an MCP client:

```
MCP client (AI agent)
        │ JSON-RPC over stdio or HTTP
        ▼
telemaco-mcp/lib.rs              tool router, owns the browser state
        │
        ▼
telemaco-browser/page.rs         navigate_with_wait
        │
        ├──► telemaco-net/client.rs        HTTP fetch
        │
        ├──► telemaco-dom/tree.rs          parse HTML into the tree
        │
        └──► telemaco-js/runtime.rs        run inline scripts
                  │
                  └──► bootstrap.js + ops.rs    DOM bindings
```

`telemaco fetch` takes the same path from `telemaco-cli/main.rs` into `telemaco-browser/page.rs`.

## Rendering flow

`telemaco-render` consumes the shared DOM and computed style state. Its
layout and text engines are Telemaco-owned forks published to crates.io and
maintained here: `mentore` (the fork of Taffy) provides the flex/grid
foundation, and `athena-text` (the fork of cosmic-text) provides text shaping
and CSS line breaking. Telemaco adds browser formatting behavior, intrinsic
replaced-element sizing, retained geometry, scrolling, and CPU-backed paint.
`telemaco-js` exposes renderer-owned geometry to DOM APIs,
and `telemaco-browser` prepares resources and owns capture: screenshots and
raster PDF output.

Layout is retained between captures and invalidated by relevant DOM, style,
viewport, scroll, animation, font, and resource changes. The same geometry
therefore drives browser APIs and paint instead of maintaining separate
measurement and screenshot models.

## Single V8 isolate

All pages in a process share one V8 isolate. The isolate is single-threaded by design.

`telemaco_js::v8_lock::global()` is a `tokio::sync::Mutex` that serializes V8 work. A handler that wants to run JS must acquire the lock first:

```rust
let _guard = telemaco_js::v8_lock::global().lock().await;
page.evaluate(expr).await
```

## Robustness

One page cannot hang or crash the process. `telemaco-js/runtime.rs` provides a V8 termination watchdog (`arm_watchdog`, `run_event_loop_bounded`) that terminates the isolate from a separate thread when synchronous work overruns a budget, because `tokio::time::timeout` cannot preempt synchronous V8. It bounds the post-load settle, the navigation event-loop pumps, and `--eval`. The complete script phase is bounded by `TELEMACO_SCRIPT_DEADLINE_MS`; enhancement modules have a shorter per-module graph-loading/evaluation budget controlled by `TELEMACO_MODULE_BUDGET_MS`, while modules mounting an empty SPA shell receive the full script deadline. `telemaco-js/cdp_watchdog.rs` is a single shared watchdog armed around synchronous V8 entry in the autonomous event-loop turn the MCP server drives, so a runaway page cannot hold the V8 lock. `op_dom` is wrapped in `catch_unwind` so a DOM-op panic degrades to a null result instead of aborting the process through V8's FFI frame, node-creating DOM ops throw a `RangeError` past a per-document node budget (`TELEMACO_MAX_DOM_NODES`), since the tree lives outside the V8 heap limit, and `telemaco-dom/tree.rs` rejects cyclic reparenting that would make tree walks loop forever. Scripted `fetch()`/XHR and module network requests are timeout-bounded (`TELEMACO_FETCH_TIMEOUT_MS`), and the one-shot `fetch` CLI has a process-level hard deadline as a final backstop.

## JS bridge

`telemaco-js/js/bootstrap.js` provides the browser globals: `document`, `window`, `navigator`, `location`, observers, fetch, indexedDB, etc.

`telemaco-js/src/ops.rs` registers Rust ops that the bootstrap calls into:

```js
Deno.core.ops.op_dom('insert_before', parentNid, refNid, newNid);
```

Adding a Web API usually means:

1. JS shim in `bootstrap.js` that exposes the API surface.
2. Rust op in `ops.rs` that performs the side effect (DOM mutation, fetch, crypto).
3. Register the op in `build_extension()`.

Worked example: [Adding a Web API](Adding-a-Web-API.md).

## Lifecycle

Lifecycle events are emitted by `telemaco-browser/lifecycle.rs` as the page transitions:

```
init → commit → domcontentloaded → load → networkidle2 → networkidle0
```

Navigation (`navigate_with_wait`, `fetch --wait-until`) blocks until the requested level is reached.

## Storage

`--storage-dir` persists cookies (`cookies.json`) and localStorage (`localStorage/<origin>.json`). Reads on process start, writes on every navigation and on graceful shutdown.

## Stealth

`--stealth` swaps the default `reqwest` client for `telemaco-net/wreq_client.rs`, which presents a real browser's TLS ClientHello, ALPN, and cipher order (a consistent Chrome fingerprint, not a randomized one) so the TLS layer matches the User-Agent and JS surfaces. It also applies the bundled tracker blocklist before any request leaves the process. Scripted `fetch()`/XHR go through the same stealth client, so subresource requests carry the same fingerprint as the navigation. `--stealth` is a global CLI flag that applies to `fetch`, `scrape`, and `mcp`.

## Workspace conventions

- One crate per layer. Cross-crate calls go through the layer above, not sideways.
- All async is `tokio` with a `LocalSet` because V8 is `!Send`.
- All DOM ops go through `op_dom` to keep the JS/Rust boundary narrow.
