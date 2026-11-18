# Review guide: Telemaco

Check only the rules whose trigger matches a path in the diff. For each finding, name the rule (R1, R2, ...) or say "outside REVIEW.md" when it is a bug no rule covers. Report real defects with a concrete failure scenario; skip anything listed under "Do not flag".

## Rules

### R1: Page-controlled work is bounded
Trigger: `crates/telemaco-render/src/**`, `crates/telemaco-dom/src/tree.rs`, `crates/telemaco-js/src/runtime.rs`, `crates/telemaco-js/js/bootstrap.js`
Check: any recursion or loop whose depth or length the page controls (DOM depth, box tree, descendants, timers, script loops) has an explicit cap or runs under the V8 termination watchdog. The reparenting guards and the `descendants()` length cap in `tree.rs` stay intact.
Why: a stack overflow is not a panic, so `catch_unwind`, the watchdog and the process deadline cannot contain it; a deep DOM aborted the whole process. (fixes: 92afe6e | source: CLAUDE.md "One page must never hang or crash a worker", "DOM reparenting guards")

### R2: Nothing unwinds into V8
Trigger: `crates/telemaco-js/src/ops.rs`, `crates/telemaco-js/src/runtime.rs`, `Cargo.toml` (`[profile.release]`)
Check: a new or changed op wraps its body in `std::panic::catch_unwind` and turns a panic into an error return; no `unwrap`/`expect`/indexing on page-supplied values inside an op; `panic = "unwind"` stays in the release profile.
Why: a panic unwinding through V8's FFI frame aborts the process instead of failing one call. (source: CLAUDE.md, the `[profile.release]` comment in Cargo.toml)

### R3: One isolate, serialized
Trigger: code that creates a `TelemacoJsRuntime`, runs JS, or spawns threads in `crates/telemaco-js/**`, `crates/telemaco-browser/**`, `crates/telemaco-mcp/**`, `crates/telemaco-cli/src/**`
Check: isolates are built under `ISOLATE_CREATE_LOCK`, on the main thread unless V8 was warmed up there first; nothing V8-related moves to a `Send` executor (the runtime is `!Send`, `LocalSet` only).
Why: two isolates set up concurrently, or a first isolate built off the main thread, crash in V8's builtin dispatch table setup. (source: CLAUDE.md "Single V8 isolate per process", runtime.rs `ISOLATE_CREATE_LOCK` comment)

### R4: Every realm carries deno_core's context state
Trigger: `crates/telemaco-js/src/frame.rs`, context or realm creation in `crates/telemaco-js/src/runtime.rs`
Check: a new V8 context (iframe realm, worker, sandbox) gets the per-context state that deno_core's isolate-wide callbacks (`promise_reject_callback` and friends) read, before any script runs in it.
Why: an iframe realm without it segfaulted the process (exit 139) on the first rejected promise, and pipelines saw an empty page with exit 0. (fixes: dc8cb88)

### R5: Evaluation is bounded and errors stay errors
Trigger: `crates/telemaco-mcp/src/lib.rs`, evaluate paths in `crates/telemaco-browser/src/page.rs` and `crates/telemaco-js/src/runtime.rs`
Check: every evaluation reachable from a tool or CLI flag has a timeout; results are size-capped before conversion to native values; a timeout or exception surfaces as an error, not as `null` or an empty string.
Why: a Tokio timeout cannot interrupt a synchronous script loop, so an unbounded evaluation could hang the server; results were converted to native values before any size cap. (fixes: c2f487d)

### R6: Every fetch passes the SSRF gate
Trigger: `crates/telemaco-net/**`, `op_fetch_url` and navigation in `crates/telemaco-js/src/ops.rs`, `crates/telemaco-browser/src/page.rs`
Check: each URL that leaves the process (navigation, subresource, JS `fetch`/XHR, every redirect hop, an interception `Continue` rewrite) goes through `validate_fetch_url`; loopback, RFC1918 and link-local stay blocked unless `--allow-private-network` or `TELEMACO_ALLOW_PRIVATE_NETWORK=1` is set.
Why: SSRF by default is a security invariant; an interception rewrite or redirect that skips the gate reaches internal hosts. (source: CLAUDE.md "SSRF by default", AGENTS.md telemaco crate notes)

### R7: Relative URLs resolve against the document base
Trigger: code that turns page-supplied hrefs or srcs into absolute URLs in `crates/telemaco-cli/src/**`, `crates/telemaco-mcp/src/**`, `crates/telemaco-browser/src/**`
Check: resolution uses the document base URL (`resolve_base_url`, honoring `<base href>`), not `page.url`.
Why: `--dump links` and `--dump assets` reported plausible URLs on the wrong origin for pages with `<base href>`. (fixes: 271cb57)

### R8: Cookie security follows Chrome
Trigger: `crates/telemaco-net/src/cookies.rs`
Check: `Secure` cookies are stored and sent for https and for loopback origins (`localhost`, `*.localhost`, `127.0.0.0/8`, `::1`), and for nothing else over plain http.
Why: local login flows lost their session because Secure cookies from a loopback dev server were stored but never sent. (fixes: ba47db4)

### R9: The installer is opt-in and leaves no husk
Trigger: `crates/telemaco-cli/src/installer/**`
Check: agent directives and the prompt hook are installed only when asked; declining or uninstalling removes any file Telemaco emptied; `targets/invariants.rs` covers a new or changed target.
Why: declining the hook left empty config files in users' agent directories, and the directives used to reach anyone who wired the server by hand. (fixes: fc264fa, 1f81293)

### R10: Gate changes land before the code they judge
Trigger: `scripts/ci/**`, `deny.toml`, `acceptance/run.py`, `.github/workflows/ci.yml`
Check: a PR that adds acceptance stages raises `MIN_STAGE_COUNT` in `scripts/ci/compare_acceptance.py`; a PR that removes stages relies on a floor already lowered on the base branch; a change to a gate script is in its own PR, because CI reads `scripts/ci/*.py` and `deny.toml` from the base SHA.
Why: a PR cannot change the gate that judges it; an exact stage count failed every PR that added stages. (fixes: dfd9a68)

### R11: Manifests and lockfile move together
Trigger: `Cargo.toml`, `crates/*/Cargo.toml`, `Cargo.lock`
Check: a dependency or feature change ships with the matching `Cargo.lock` in the same commit; `deno_core` stays pinned to the published `telemaco-deno-core` fork; no new dependency without a stated reason.
Why: a manifest without its lockfile breaks the next build; the deno_core fork carries a patch that a blind bump drops. (source: CLAUDE.md, project memory)

### R12: No hostname-specific behavior
Trigger: `crates/telemaco-render/**`, `crates/telemaco-browser/**`, `crates/telemaco-net/**`, `crates/telemaco-js/**`
Check: no layout, style, script or resource behavior keyed on a specific hostname or site.
Why: site-specific patches hide engine bugs and break on the next site. (source: CLAUDE.md "Rendering verification")

### R13: No em or en dashes in prose
Trigger: `*.md`, `index.html`, comments and doc comments in changed code
Check: added lines contain no em dash (U+2014) or en dash (U+2013).
Why: project and owner convention. (source: CLAUDE.md "Conventions")

## Do not flag

- Formatting and import order: the tree is not rustfmt-clean on purpose; match surrounding style instead of running `cargo fmt` (CLAUDE.md).
- `cdp_watchdog` and other CDP-era names in `telemaco-js`: the module is still used by the MCP event-loop path; the name predates the CDP removal.
- Comments describing Puppeteer or Playwright compatibility in engine code: they document why a behavior exists.
- `cfg(windows)` code paths without Windows release builds: users can still build from source.
- `RUST_TEST_THREADS = "1"` in `.cargo/config.toml` and the use of `cargo nextest`: the engine holds one V8 isolate per process.
