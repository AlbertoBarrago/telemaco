# Changelog

All notable changes to Telemaco are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.2.2]: https://github.com/AlbertoBarrago/telemaco/releases/tag/v0.2.2
[0.2.1]: https://github.com/AlbertoBarrago/telemaco/releases/tag/v0.2.1
