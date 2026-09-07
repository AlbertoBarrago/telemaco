# telemaco-cdp

Chrome DevTools Protocol server for the
[Telemaco](https://github.com/AlbertoBarrago/telemaco) headless browser,
compatible with Puppeteer and Playwright.

## Place in the workspace

The protocol layer. It exposes a WebSocket CDP endpoint (`server.rs`), routes
messages to per-domain handlers (`dispatch.rs`, `domains/`) and drives
`telemaco-browser` pages behind them. It sits above `telemaco-browser`,
`telemaco-js`, `telemaco-dom` and `telemaco-net`; the only thing above it is
`telemaco-cli`, whose `telemaco serve` command starts it.

## Features

| Feature | Effect |
|---------|--------|
| `stealth` | Enables `telemaco-browser/stealth`: wreq/BoringSSL transport and matching browser identity for served pages. |
| `render` | Enables `telemaco-browser/render` and pulls in `image` and `png`, which is what makes `Page.captureScreenshot` and screencasting available. |

## Usage

```rust,no_run
#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    // Listens on ws://127.0.0.1:9222 and serves the usual /json endpoints.
    telemaco_cdp::start(9222).await
}
```

`start_with_options`, `start_with_full_options`, `start_with_host` and
`start_with_host_and_security` add proxy, stealth, user agent, storage
directory, bind host and connection limits (`DEFAULT_MAX_CONNECTIONS`).

The server must run inside a tokio `LocalSet`, since V8 and the DOM are not
`Send`.

## Invariants

- **`canAccessOpener` must be present in every `TargetInfo` payload.** Strict
  clients (chromiumoxide) panic without it.
- Managed page sessions use the id `"{targetId}-session"`; explicitly flattened
  attachments get distinct session ids, so Playwright and Puppeteer can open raw
  page sessions.
- One V8 isolate exists per process, so a per-connection lock (`ctx.v8_lock`)
  keeps handlers contiguous. Handlers that run JS must take it.

Part of [Telemaco](https://github.com/AlbertoBarrago/telemaco).
