# Acceptance gate

The behavioral gate for telemaco. It drives the release binary against local
fixtures and reports one pass/fail plus a median latency for each of its 40
stages. Everything is served from this machine, so a run is deterministic and
needs no network.

```bash
CARGO_INCREMENTAL=0 cargo build --release -p telemaco-cli --bins --features render
TELEMACO_BIN=./target/release/telemaco python3 acceptance/run.py
```

Expected result: **40/40**. The exit code is non-zero when any stage fails.

## Why it exists

The unit suite checks the pieces and passes even when the assembled engine is
broken. The `browser_markdown` bug that shipped in this repo is the shape of
it: every function was correct, and the tool still returned nothing, because
the failure lived in how they combined.

Its first run found a real one. `--dump links` resolved relative hrefs against
the document URL instead of the document base URL, so a page with
`<base href="/sub/">` reported the wrong targets. The engine had the correct
logic; the CLI could not reach it.

## Running

```bash
python3 acceptance/run.py                       # all stages, one run each
python3 acceptance/run.py --runs 3 --warmup 1   # steadier timings
python3 acceptance/run.py --json                # the CI contract
python3 acceptance/run.py --filter js.          # only the JS stages
```

`TELEMACO_BIN` selects the binary and defaults to `target/release/telemaco`.
Stages that need rendering assume a `--features render` build.

## The contract

`--json` emits exactly:

```json
{"results": [{"name": "dom.entities", "pass": true, "median_ms": 30.3}]}
```

`scripts/ci/compare_acceptance.py` consumes two of these, one per revision, and
fails when a stage that passed on the base no longer passes. Latency is
reported, never enforced: CI runners are too noisy for a timing threshold to
mean anything on its own.

**Stage names are part of that contract.** Renaming one makes the comparison
report a missing stage instead of a regression, which reads as a broken run
rather than the intended signal.

## Stages

| Group | Count | Covers |
|---|---|---|
| `dom.` | 5 | malformed markup, entities, non-UTF-8 charset, `<base href>`, nested tables |
| `js.` | 7 | DOM injection, timers, `fetch`, XHR, async/await, custom elements, DOM mutation order |
| `extract.` | 6 | text, html, links, markdown, binary passthrough, asset enumeration |
| `nav.` | 5 | redirect chains, meta refresh, 404, timeout, waiting for a selector |
| `state.` | 4 | cookies, `--storage-dir`, localStorage, the `--eval` IIFE workaround |
| `security.` | 3 | the SSRF gate closed and open, robots.txt |
| `render.` | 3 | screenshot, viewport height, PDF |
| `protocol.` | 4 | the CDP endpoint, MCP tool list, MCP navigate and read, MCP fill and select |
| `speed.` | 3 | static page, scripted page, parallel scrape |

## Characterization stages

Two stages pin behavior rather than assert what a browser ought to do, and say
so in a comment at the assertion:

- `nav.meta-refresh` pins that telemaco does **not** follow
  `<meta http-equiv="refresh">`; it returns the source document. This is a real
  gap against browser behavior, recorded here so it cannot change unnoticed.
  Implementing the refresh turns the stage red on purpose, and the expectation
  moves to the target page in the same change.

Keep the comment when you touch one. A pinned limitation with no explanation
reads as an assertion that the limitation is correct.

## Adding a stage

Write a function taking the fixture base URL and returning a bool, then add it
to `STAGES` with a stable name. Assert on observable behavior, never on
wording the CLI is free to change, and bump `EXPECTED_STAGE_COUNT` in
`scripts/ci/compare_acceptance.py` to match the new total.

A stage that cannot fail is worse than no stage: it costs time on every run and
reports success regardless. Before adding one, break the thing it covers on
purpose and confirm it goes red.
