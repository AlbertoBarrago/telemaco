# Changelog

All notable changes to Telemaco are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.5] - 2026-10-19

### Fixed

- **`Secure` cookies did not work on a local dev server.** The cookie jar
  treated only `https` as secure, so a `Secure` session cookie set by
  `http://localhost` was stored but never sent back or shown in
  `document.cookie`, and local login flows lost their session on the next
  request. Like Chrome, loopback hosts (`localhost`, `*.localhost`,
  `127.0.0.0/8`, `::1`) now count as secure. A new acceptance stage covers
  it; the suite is now 46 stages.

## [0.2.4] - 2026-10-19

### Added

- **A per-document DOM node budget.** Node-creating DOM calls
  (`createElement`, `cloneNode`, `innerHTML`, `document.write`, and similar)
  throw a catchable `RangeError` once a document holds 500,000 live nodes.
  The tree lives outside the V8 heap limit, so a script that grew the DOM
  without bound reached 4.6 GB of resident memory before the script watchdog
  stopped it; it now stops at about 550 MB in 2.4 seconds. Configure it with
  `TELEMACO_MAX_DOM_NODES`.
- **Hostile-page acceptance stages.** Four `hostile.*` stages (a node bomb,
  `while (true)`, a zero-delay timer storm, an endless response body) turn
  the robustness invariants into a gate. The suite is now 45 stages.

### Fixed

- **`fetch --timeout` did not stop a synchronous script.** The navigation
  timeout cannot preempt V8, and the script watchdog ignored it in favor of
  `TELEMACO_SCRIPT_DEADLINE_MS` (30 seconds), so a `while (true)` page under
  `--timeout 3` was killed by the process hard deadline after 18 seconds with
  no output. The script deadline is now capped at the page's navigation
  timeout, and the page returns in about 4 seconds. This also applies to the
  navigation timeouts set by CDP and MCP clients.
- **`rustls` updated to 0.23.46** for RUSTSEC-2026-0346, RUSTSEC-2026-0347,
  RUSTSEC-2026-0348 and RUSTSEC-2026-0349.
- **CI failed every pull request that added acceptance stages.** The
  comparison script, read from the base revision, required exactly 41
  stages; the count is now a floor.

## [0.2.3] - 2026-09-29

### Fixed

- **`cargo clippy` failed to compile on `telemaco-js` and every crate that
  depends on it.** The import map prefix match was a loop that returned on its
  first iteration, which clippy's deny-by-default `never_loop` lint rejects.
  It now uses `find`; behavior is unchanged (the most specific prefix still
  wins), and a new test pins that rule, including after a merge
  ([#7](https://github.com/AlbertoBarrago/telemaco/issues/7)).

## [0.2.2] - 2026-09-28

### Added

- **`telemaco remote`: manage another machine over Tailcat.** A new
  `telemaco-remote` crate and CLI subcommand carry a small versioned protocol
  over [Tailcat](https://github.com/tailscale/tailcat) (end-to-end WireGuard,
  NAT traversal, DERP fallback), with no public inbound port, no VPN and no
  Tailscale account. Tailcat is an optional runtime dependency, driven as the
  external `tailcat` CLI (v0.7.0 or newer); every other command works without
  it. See [Remote over Tailcat](docs/Remote-over-Tailcat.md).
  - `remote serve` exposes a Telemaco agent through Tailcat's `exec` service
    and prints its address. Ephemeral key by default; `--allow` restricts
    client keys; `--key` uses a saved key.
  - `remote status` and `remote ping` report host, OS, versions, forwarded
    ports and whether the path is direct or relayed.
  - `remote exec <addr> -- <program> [args]...` runs a program without a
    shell, streams stdout and stderr, and exits with its status. Supports
    `--cwd`, `--env`, `--timeout`; Ctrl-C kills the remote program. Off unless
    the server passes `--allow-exec`, which is refused with a saved key and no
    `--allow`.
  - `remote forward <addr> <port>` forwards a port the server exposes with
    `--forward-port`, listening on `127.0.0.1` by default.
  - The tailcat address is treated as a credential: redacted in every log and
    error message, printed in full only once by `remote serve`.
- **Landing page:** a Remote section describing the Tailcat transport.

## [0.2.1] - 2026-09-09

### Added

- **Custom Elements v1: direct construction with `new Ctor()`.** The `Element`
  constructor now handles creating a custom element outside
  `document.createElement` (the pattern used by LWC, Lit and Stencil): it
  allocates a real node from the registered name via
  `customElements.getName(new.target)`. An unregistered class throws
  `TypeError: Illegal constructor` (spec-correct behavior).
- **Custom Elements v1: `attributeChangedCallback` lifecycle.** Implemented the
  `observedAttributes`/`attributeChangedCallback` hook, wired into
  `setAttribute`, `removeAttribute` and the upgrade step. It was not
  implemented at all before.

### Fixed

- **React/Next.js hydration false alarms.** The `Minified React error #418`
  (an SSR hydration mismatch, not an engine crash) is now logged as a warning
  instead of an error. The page still renders normally.

### Known issue

- **Salesforce documentation pages (LWR/LWC) do not render their content.** On
  `developer.salesforce.com/docs/...` the `doc-xml-content` component mounts an
  empty shadow root: the Lightning Web Runtime module-loading chain never gets
  to request the content. The Custom Elements fixes above resolve the crash and
  the lifecycle hook, but not the Apex content loading, which remains
  unresolved. The rest of the engine works normally.

[0.2.5]: https://github.com/AlbertoBarrago/telemaco/releases/tag/v0.2.5
[0.2.4]: https://github.com/AlbertoBarrago/telemaco/releases/tag/v0.2.4
[0.2.3]: https://github.com/AlbertoBarrago/telemaco/releases/tag/v0.2.3
[0.2.2]: https://github.com/AlbertoBarrago/telemaco/releases/tag/v0.2.2
[0.2.1]: https://github.com/AlbertoBarrago/telemaco/releases/tag/v0.2.1
