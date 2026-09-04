# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Telemaco is a headless browser engine in Rust: real JavaScript through V8
(`deno_core`), a real DOM tree, its own layout and paint pipeline, and the
Chrome DevTools Protocol. It is a drop-in replacement for headless Chrome with
Puppeteer and Playwright, targeting web scraping and AI-agent automation.

**Read `AGENTS.md` first.** It is the authoritative, detailed guide: build and
test commands, architecture, conventions, rendering verification, gotchas, and
robustness invariants. This file is a condensed pointer, not a replacement.

## Build

```bash
# Rendering (default for release archives)
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo build --release -p telemaco-cli --bins --features render

# Rendering + stealth (wreq/BoringSSL transport, fingerprint, tracker blocklist)
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo build --release -p telemaco-cli --bins --features render,stealth

# No rendering
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo build --release -p telemaco-cli --bins --no-default-features
```

- The first build compiles V8 from source (~5 min, a few GB); incremental builds
  are seconds. Scope iteration to one crate with `-p telemaco-cli` to avoid
  re-linking the whole workspace.
- Stealth needs `cmake` (BoringSSL builds through CMake). If the vendored
  OpenSSL build hits an AVX-512 assembler error, use `OPENSSL_NO_VENDOR=1`.

## Test

Use **`cargo nextest`, not `cargo test`**: the engine holds a single V8 isolate
per process, and nextest runs each test in its own process.

```bash
cargo nextest run --release --features render -p <crate>
cargo nextest run --release --features render --no-fail-fast
```

The authoritative behavioral gate is the acceptance suite in `acceptance/`
(41 stages, must stay 41/41):

```bash
TELEMACO_BIN=./target/release/telemaco python3 acceptance/run.py
```

Deterministic and offline: it serves its own fixtures on a runtime-chosen port.

Do not bulk-run `cargo fmt`: the tree is not rustfmt-clean. Match surrounding
style in the files you edit.

## Architecture

Workspace of nine crates, one layer per crate; cross-crate calls go through the
layer above, not sideways. All async is `tokio` with a `LocalSet` because V8 is
`!Send`. All DOM ops go through `op_dom` to keep the JS/Rust boundary narrow.

| Crate | Role |
|-------|------|
| `telemaco-cli` | CLI: `fetch`, `serve` (CDP server), `scrape`, `mcp` |
| `telemaco-cdp` | Chrome DevTools Protocol server (WebSocket) |
| `telemaco-js` | V8/`deno_core` runtime; `js/bootstrap.js` DOM shim + `src/ops.rs` bridge |
| `telemaco-dom` | DOM tree (`src/tree.rs`) |
| `telemaco-net` | HTTP client, stealth client, cookie jar, robots cache, tracker blocklist |
| `telemaco-browser` | The `Page` type, navigation, JS evaluation |
| `telemaco-render` | Selector cascade, retained layout, paint, screenshots, PDF |
| `telemaco-mcp` | Stateful MCP automation tools |
| `telemaco` | Embeddable Rust library API |

Key invariants (see `AGENTS.md` for detail):

- **Single V8 isolate per process**, serialized by `telemaco_js::v8_lock::global()`
  (a `tokio::sync::Mutex`). Acquire it before running JS.
- **One page must never hang or crash a worker**: V8 termination watchdog
  (`arm_watchdog`/`disarm_watchdog` in `runtime.rs`), process-level hard
  deadline, and `panic = "unwind"` pinned in the release profile so
  `catch_unwind` works. Keep ops panic-safe: never unwind into V8's FFI frame.
- **The DOM reparenting guards in `tree.rs` are load-bearing** (reject cycles;
  keep the `descendants()` length cap).
- **SSRF by default**: loopback / RFC1918 / link-local fetches are blocked. Use
  `--allow-private-network` (or `TELEMACO_ALLOW_PRIVATE_NETWORK=1`) for local
  testing.
- **`canAccessOpener` must be in every `TargetInfo` payload**, or strict CDP
  clients (chromiumoxide) panic.
- **Multi-statement `--eval` starting with `const` returns `null`** (V8 empty
  completion value). Wrap in an IIFE.

## Rendering verification

Use deterministic fixtures before real sites; put generated output in a
disposable directory outside the repo. See `AGENTS.md` for the exact harness
commands and comparison methodology. Never add hostname-specific layout, style,
or resource behavior. Do not commit generated screenshots or reports.

## Conventions

- Performance is a hard constraint (~12x faster, ~6x less memory than headless
  Chrome on framework pages). Keep native Rust fast paths; add a JS fallback
  only for real spec edge cases.
- Commits/PRs/comments: short and factual, no em dashes, no AI filler.
- The `skills/telemaco/SKILL.md` skill documents operating and validating the
  binary (build variants, stealth, CDP, MCP, visual comparison).
