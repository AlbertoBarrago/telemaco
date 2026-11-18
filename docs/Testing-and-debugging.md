## Test suites

### Rust unit and integration

```bash
cargo nextest run --release --features render --no-fail-fast
```

Crate-scoped:

```bash
cargo nextest run --release --features render -p telemaco-browser
cargo nextest run --release --features render -p telemaco-mcp
```

By name:

```bash
cargo nextest run --release --features render runtime_click_submit_prevent_default
```

Use `cargo nextest`, not `cargo test`. Runtime tests require process isolation
because the engine owns one V8 isolate per process. Render tests must run in
release mode; debug builds are not a fidelity or performance gate.

### Engine integration tests

`crates/telemaco-browser/tests/*.rs` drive a real `Page` against an in-process HTTP server: `BrowserContext` and `Page::new`, then `navigate`, `evaluate`, and `settle`. Start from an existing file there for the server and setup helpers.

## Logging

```bash
RUST_LOG=telemaco=info  telemaco fetch https://example.com
RUST_LOG=telemaco=debug telemaco fetch https://example.com
RUST_LOG=telemaco_mcp=trace,telemaco_browser=debug telemaco mcp
```

Logs go to stderr.

`--verbose` on any subcommand is equivalent to `RUST_LOG=telemaco=info`.

## Common failure modes

### `V8_Fatal: heap->isolate() == Isolate::TryGetCurrent()`

Two pages tried to use V8 concurrently. The `v8_lock` was bypassed, or a handler suspended a JS runtime while another isolate was entered. Search for direct `JsRuntime` access outside the lock.

### Test hangs

A handler is awaiting something that never resolves. Run with `RUST_LOG=telemaco=trace` and check the last log line before the hang.

## Reproducing user bug reports

The integration suite in `tests/test_all.py` is the fastest path from a one-line repro to a regression test. Add the failing case as a new test function, get it failing, then fix.

### Rendering regressions

Start with the committed deterministic fixtures, then use the representative
real-site suite at both the top and bottom of pages:

```bash
RUN_ROOT="$(mktemp -d)"
TELEMACO_BIN=./target/release/telemaco render-repros/run.sh "$RUN_ROOT/fixtures"
TELEMACO_BIN=./target/release/telemaco render-repros/representative-suite/run.sh "$RUN_ROOT/top"
TELEMACO_BIN=./target/release/telemaco render-repros/representative-suite/run.sh "$RUN_ROOT/bottom" bottom
```

Set `BASELINE_BIN` or `CHROMIUM_BIN` when producing paired captures. Keep the
viewport, user agent, settle policy, scroll position, animation sample, and
capture boundary identical. A pixel-distance score is a regression tripwire,
not a verdict: verify both engines succeeded and produced nonblank output,
then inspect missing resources, geometry, structural edges, and a reduced
fixture. Do not add hostname-specific render branches.

## Profiling

CPU with `perf` and a flamegraph:

```bash
cargo build --release --features render
perf record -F 99 -g -- ./target/release/telemaco fetch https://heavy-spa.example
perf script | flamegraph.pl > flame.svg
```

Memory with heaptrack:

```bash
heaptrack ./target/release/telemaco mcp
```

Tokio task inspection:

```bash
RUSTFLAGS="--cfg tokio_unstable" cargo build --release --features render
./target/release/telemaco mcp
# in another shell
tokio-console
```

Requires the workspace `tokio` dependency to be built with the `tracing` feature; not enabled by default, add it in the relevant `Cargo.toml` before profiling.
