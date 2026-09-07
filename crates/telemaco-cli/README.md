# telemaco-cli

Command line interface for
[Telemaco](https://github.com/AlbertoBarrago/telemaco): `fetch`, CDP server,
scraping, MCP and the agent installer.

## Place in the workspace

The top layer of the workspace, and the crate that produces the `telemaco`
binary. It wires the layers below into commands: `telemaco-browser` for pages,
`telemaco-cdp` for the DevTools Protocol server, `telemaco-mcp` for the agent
tools, `telemaco-net`, `telemaco-js` and `telemaco-dom` underneath. Nothing
depends on this crate. It also ships a second binary, `telemaco-worker`, used by
`scrape` for per-URL worker processes.

## Install

```bash
cargo install telemaco-cli --features render
```

The first build compiles V8 from source (several minutes and a few GB of disk).
Add `stealth` for the wreq/BoringSSL transport, which needs `cmake` installed.

## Commands

| Command | What it does |
|---------|--------------|
| `fetch <url>` | Load one page. `--dump assets\|html\|text\|links\|markdown\|original\|cookies`, `--eval <JS>`, `--screenshot <PNG>`. |
| `serve` | Start the CDP server, for Puppeteer and Playwright. |
| `scrape <urls...>` | Batch fetch across worker processes, `--concurrency N`. |
| `mcp` | Run the MCP server on stdio, for AI agents. |
| `install` / `uninstall` | Wire the MCP server, an instructions block and a prompt hook into 15 coding agents. Global by default, or `--folder <dir>`. |

`--proxy`, `--stealth` and `--allow-private-network` are global flags, valid
before or after the subcommand.

## Features

| Feature | Effect |
|---------|--------|
| `stealth` | Enables stealth in `telemaco-browser`, `telemaco-net` and `telemaco-mcp`: wreq/BoringSSL transport, matching browser identity, tracker blocklist. |
| `render` | Enables rendering in `telemaco-browser`, `telemaco-cdp` and `telemaco-mcp`: screenshots, PDF and CDP screen capture. Without it `--screenshot` says so and exits. |

## Notes

- Loopback and RFC1918 targets are blocked by default (SSRF guard). Use
  `--allow-private-network` only for deliberate local testing.
- A multi-statement `--eval` starting with `const` returns `null`, because V8
  gives `const` an empty completion value. Wrap the snippet in an IIFE:
  `(function(){ ...; return result; })()`.
- The CLI applies a process-level hard deadline as a backstop, so one page can
  never hang a run.

Part of [Telemaco](https://github.com/AlbertoBarrago/telemaco).
