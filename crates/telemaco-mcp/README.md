# telemaco-mcp

Model Context Protocol server exposing
[Telemaco](https://github.com/AlbertoBarrago/telemaco) browser automation to AI
agents.

## Place in the workspace

The agent-facing layer. It keeps browser state across calls (tabs, the active
tab, console messages, and the element-reference table produced by
`browser_snapshot`) and answers JSON-RPC on stdio with the tool set an agent
uses to navigate, read and fill pages. It sits above `telemaco-browser`,
`telemaco-dom` and `telemaco-net`. `telemaco-cli` is the layer above it and
starts it with `telemaco mcp`.

## Features

| Feature | Effect |
|---------|--------|
| `stealth` | Enables `telemaco-browser/stealth` and `telemaco-net/stealth`: the wreq/BoringSSL transport and matching browser identity. |
| `render` | Enables `telemaco-browser/render` and pulls in `base64`, which is what makes the `browser_screenshot` and `browser_pdf` tools available. Streaming screencasts stay CDP-only. |

## Usage

Normally you do not run this crate directly: use `telemaco mcp` from
`telemaco-cli`, or wire it into an agent with `telemaco install`. Embedded, the
entry point is a single async function that serves JSON-RPC on stdin/stdout
until the client disconnects.

```rust,no_run
use telemaco_mcp::config::ExtractionLimits;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    telemaco_mcp::run(
        None,                          // proxy
        None,                          // user agent override
        false,                         // stealth
        ExtractionLimits::default(),   // per-tool output caps
        false,                         // agent directives in `initialize`
    )
    .await
}
```

Run it inside a tokio `LocalSet`: pages are not `Send`.

## Invariants

- **stdout is the protocol stream.** Anything printed there that is not JSON-RPC
  corrupts the session; diagnostics go to stderr.
- Snapshot element refs (`e3` and similar) are stable only within one snapshot.
  The table is cleared on every navigation and tab switch.
- The `telemaco-net` SSRF guard applies here too: loopback and RFC1918 targets
  are refused unless private network access is explicitly enabled.

Part of [Telemaco](https://github.com/AlbertoBarrago/telemaco).
