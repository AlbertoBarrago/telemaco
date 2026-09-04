<div align="center">
  <img src="assets/icon.png?v=2" alt="Telemaco" width="80" />

  <h1>Telemaco</h1>

  <p>
    <strong>Control the web with precision.</strong><br>
    A headless browser engine in Rust, built for web scraping and AI agents.<br>
    Real JavaScript, real DOM, native layout and paint. No Chromium required.
  </p>
</div>

---

Telemaco runs real JavaScript through V8 (`deno_core`), keeps a real DOM tree,
owns its layout and paint pipeline, speaks the Chrome DevTools Protocol, and
acts as a drop-in replacement for headless Chrome with Puppeteer and
Playwright. Rendering and stealth are first-class capabilities. It targets web
scraping and AI agent automation.

## Why Telemaco over headless Chrome?

| Metric       | Telemaco      | Headless Chrome |
|--------------|---------------|-----------------|
| Memory       | **30 MB**     | 200+ MB         |
| Binary size  | **70 MB**     | 300+ MB         |
| Page load    | **85 ms**     | ~500 ms         |
| Startup      | **Instant**   | ~2s             |
| Anti-detect  | **Built-in**  | None            |
| Puppeteer    | **Yes**       | Yes             |
| Playwright   | **Yes**       | Yes             |

Roughly 12x faster page loads and 6x less memory than headless Chrome on
framework pages, with the same CDP automation surface.

## Highlights

- **Native rendering**: CSS layout and paint, viewport and full-page
  screenshots, activity-driven CDP screencasting, and PDF export. No Chromium,
  no WebView.
- **Stealth mode**: wreq/BoringSSL transport, per-session fingerprint
  randomization, consistent browser identity, and a tracker blocklist.
  Rendering stays fully available when stealth is on.
- **CDP compatible**: `telemaco serve` speaks the Chrome DevTools Protocol, so
  Puppeteer, Playwright, and chromiumoxide connect out of the box.
- **MCP server**: stateful browser automation tools for Claude Desktop, Cursor,
  and any MCP client, over stdio or HTTP.
- **Hardened by default**: SSRF guards on loopback, RFC1918, and link-local
  targets; a V8 termination watchdog per page; process-level hard deadlines, so
  one bad page can never hang a worker.

## Install

### Prebuilt binaries

Grab the latest archive from
[Releases](https://github.com/AlbertoBarrago/telemaco/releases):

```bash
# Linux x86_64
curl -LO https://github.com/AlbertoBarrago/telemaco/releases/latest/download/telemaco-x86_64-linux.tar.gz
tar xzf telemaco-x86_64-linux.tar.gz
./telemaco fetch https://example.com --eval "document.title"
```

Release archives include both `telemaco` and `telemaco-worker`; keep them in
the same directory for the parallel `scrape` command.

| Archive suffix        | Rendering | Stealth transport |
|-----------------------|-----------|-------------------|
| none                  | Yes       | No                |
| `-stealth`            | Yes       | Yes               |
| `-no-render`          | No        | No                |
| `-no-render-stealth`  | No        | Yes               |

### Docker

```bash
docker build -t telemaco .
docker run -d --name telemaco -p 127.0.0.1:9222:9222 telemaco
```

Multi-stage build on `distroless/cc`: no shell, no package manager.

### Build from source

```bash
git clone https://github.com/AlbertoBarrago/telemaco.git
cd telemaco

# Rendering
CARGO_INCREMENTAL=0 cargo build --release -p telemaco-cli --bins --features render

# Rendering and stealth
CARGO_INCREMENTAL=0 cargo build --release -p telemaco-cli --bins --features render,stealth

# No rendering
CARGO_INCREMENTAL=0 cargo build --release -p telemaco-cli --bins --no-default-features

# No rendering, with stealth
CARGO_INCREMENTAL=0 cargo build --release -p telemaco-cli --bins --no-default-features --features stealth
```

Requires Rust 1.75+ ([rustup.rs](https://rustup.rs)). The first build compiles
V8 from source: about 5 minutes and a few GB of disk, cached afterwards. The
stealth build also compiles BoringSSL, which needs CMake and Clang. On
Ubuntu/Debian:

```bash
sudo apt-get install build-essential cmake clang libclang-dev llvm-dev
```

## Quick start

### Fetch a page

```bash
# Page title
telemaco fetch https://example.com --eval "document.title"

# Extract all links
telemaco fetch https://example.com --dump links

# Render JavaScript and dump HTML
telemaco fetch https://news.ycombinator.com --dump html

# Write output to a file
telemaco fetch https://example.com --dump text --output page.txt

# Stream the raw response body verbatim (binary-safe, bypasses the JS/DOM layer)
telemaco fetch https://picsum.photos/200/300 --dump original > photo.jpg

# List every sub-resource URL the page would fetch (NDJSON)
telemaco fetch https://example.com --dump assets

# Fetch through an HTTP or SOCKS proxy
telemaco --proxy socks5://127.0.0.1:1080 fetch https://example.com --dump text

# Wait for dynamic content, bound navigation time
telemaco fetch https://example.com --wait-until networkidle0 --timeout 10

# Capture the settled page as PNG
telemaco fetch https://example.com --screenshot page.png
```

Dump modes: `assets`, `html`, `text`, `links`, `markdown`, `original`,
`cookies`.

### Localhost and LAN dev servers

Fetches to private and internal IPs are blocked by default (SSRF protection).
For local testing pass `--allow-private-network` or set
`TELEMACO_ALLOW_PRIVATE_NETWORK=1`:

```bash
telemaco fetch http://127.0.0.1:3000 --allow-private-network --dump text

# Works on any subcommand, for example the CDP server for local automation:
telemaco serve --port 9222 --allow-private-network
```

The full allow/deny rules (including DNS-resolution-time checks) are in
[docs/Environment-variables.md](docs/Environment-variables.md).

### Scrape in parallel

```bash
telemaco scrape url1 url2 url3 ... \
  --concurrency 25 \
  --eval "document.querySelector('h1').textContent" \
  --format json

# Suppress scrape progress on stderr for script-friendly output
telemaco scrape https://example.com --quiet --format json

# Workers inherit the global proxy
telemaco --proxy http://127.0.0.1:8080 scrape https://example.com https://news.ycombinator.com
```

### Drive it with Puppeteer or Playwright

```bash
telemaco serve --port 9222
```

```javascript
// Puppeteer
import puppeteer from 'puppeteer-core';

const browser = await puppeteer.connect({
  browserWSEndpoint: 'ws://127.0.0.1:9222/devtools/browser',
});

const page = await browser.newPage();
await page.goto('https://news.ycombinator.com');

const stories = await page.evaluate(() =>
  Array.from(document.querySelectorAll('.titleline > a'))
    .map(a => ({ title: a.textContent, url: a.href }))
);
console.log(stories);

await browser.disconnect();
```

```javascript
// Playwright
import { chromium } from 'playwright-core';

const browser = await chromium.connectOverCDP({
  endpointURL: 'ws://127.0.0.1:9222',
});

const page = await browser.newContext().then(ctx => ctx.newPage());
await page.goto('https://en.wikipedia.org/wiki/Web_scraping');
console.log(await page.title());

await browser.close();
```

Rendering-enabled builds add `page.screenshot()` (viewport and full page) and
`page.pdf()`:

```javascript
await page.setViewport({ width: 1440, height: 1000 });
await page.goto('https://example.com', { waitUntil: 'load' });
await page.screenshot({ path: 'page.png', fullPage: true });
await page.pdf({ path: 'page.pdf', format: 'A4', printBackground: true });
```

## MCP (Model Context Protocol)

Telemaco ships an MCP server that exposes browser automation tools to AI
agents (Claude Desktop, Cursor, and other MCP clients).

```bash
telemaco mcp                      # stdio, for clients that launch a subprocess
telemaco mcp --http --port 8080   # HTTP, endpoint: http://127.0.0.1:8080/mcp
```

Claude Desktop config:

```json
{
  "mcpServers": {
    "telemaco": { "command": "telemaco", "args": ["mcp"] }
  }
}
```

Tools: `browser_navigate`, `browser_snapshot`, `browser_screenshot`,
`browser_pdf`, `browser_click`, `browser_fill`, `browser_type`,
`browser_press_key`, `browser_select_option`, `browser_evaluate`,
`browser_wait_for`, `browser_network_requests`, `browser_console_messages`,
`browser_close`. Render-enabled builds expose `browser_screenshot` and
`browser_pdf`; streaming screencasts remain CDP-only.

## CLI reference

### `telemaco fetch <URL>`

| Flag | Default | Description |
|------|---------|-------------|
| `--dump` | `text` | `assets`, `html`, `text`, `links`, `markdown`, `original`, `cookies` |
| `--eval` | — | JS expression evaluated on the page |
| `--wait-until` | `load` | `load`, `domcontentloaded`, `networkidle0` |
| `--timeout` | `30` | Maximum navigation time in seconds |
| `--wait` | adaptive, up to `5` | Post-load settling; an explicit value is a fixed delay in seconds |
| `--selector` | — | Wait for a CSS selector |
| `-s`, `--screenshot` | — | Write a PNG screenshot (render-enabled build) |
| `--stealth` | off | Anti-detection mode |
| `--output` | — | Write dump or eval output to a file |
| `--proxy` | — | Inherited global HTTP/SOCKS5 proxy URL |

Global flags `--proxy`, `--stealth`, and `--allow-private-network` are valid
before or after the subcommand and apply to `fetch`, `serve`, `scrape`, and
`mcp`.

### `telemaco scrape <URL...>`

| Flag | Default | Description |
|------|---------|-------------|
| `--concurrency` | `10` | Parallel workers |
| `--eval` | — | JS expression per page |
| `--format` | `json` | `json` or `text` |
| `--quiet` | off | Suppress scrape progress on stderr |

## Stealth mode

Build with `--features render,stealth`, then enable at runtime with the global
`--stealth` flag. Stealth adds the wreq/BoringSSL transport, per-session
fingerprint randomization (GPU, screen, canvas, audio, battery), realistic
`navigator.userAgentData` (high-entropy values), trusted dispatched events,
and a tracker blocklist. The stealth build retains the complete rendering
surface: screenshot, screencast, PDF, CDP, and MCP all keep working.

## Architecture

| Crate | Role |
|-------|------|
| `telemaco-cli` | CLI: `fetch`, `serve` (CDP server), `scrape`, `mcp` |
| `telemaco-cdp` | Chrome DevTools Protocol server (WebSocket) |
| `telemaco-js` | V8/`deno_core` runtime and DOM ops bridge |
| `telemaco-dom` | DOM tree |
| `telemaco-net` | HTTP client, stealth transport, cookie jar, robots cache, tracker blocklist |
| `telemaco-browser` | The `Page` type, navigation, JS evaluation |
| `telemaco-render` | Selector cascade, retained layout, paint, screenshots, PDF |
| `telemaco-mcp` | Stateful MCP automation tools |
| `telemaco` | Embeddable Rust library API |

## Testing

Run tests with `cargo nextest`, not `cargo test`: the engine holds a single V8
isolate per process, and nextest runs each test in its own process.

```bash
cargo nextest run --release --features render -p <crate>
cargo nextest run --release --features render --no-fail-fast
```

Beyond the unit suite there is an offline smoke battery covering the CLI, CDP,
and MCP surfaces end to end against a local fixture; it lives in
[GUIDA-AVVIO.md](GUIDA-AVVIO.md) section 9.1. Rendering changes additionally go
through `render-repros/run.sh`, which draws 64 deterministic fixtures.

Earlier revisions pointed at an obstacle course in a companion
`telemaco-benchmark` repository. That repository no longer exists.

## Proxies for production

For production scraping, residential or ISP IPs usually beat datacenter
addresses. We use [NodeMaven](https://go.nodemaven.com/telemacoagentaugust):
residential and ISP proxies, sticky sessions, and per-country targeting
through the proxy username. Route Telemaco through it with the global
`--proxy` flag. Discount codes: `TELEMACO35` (35% off mobile and residential),
`TELEMACO40` (40% off ISP and static).
