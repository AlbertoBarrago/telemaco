# Telemaco

Control the web with precision. A headless browser engine in Rust, built for
web scraping and AI agents. Real JavaScript, real DOM, native layout and
paint. No Chromium required.

## Quick start

```bash
docker pull albz222/telemaco:latest
docker run -d --name telemaco -p 127.0.0.1:9222:9222 albz222/telemaco:latest
```

The container runs the CDP server on port 9222, so Puppeteer and Playwright
connect to `ws://127.0.0.1:9222` with nothing else to set up.

Run a one-off command instead of the server by overriding the entrypoint:

```bash
docker run --rm --entrypoint /telemaco albz222/telemaco:latest \
  fetch https://example.com --dump markdown
```

## Why Telemaco over headless Chrome?

| Metric       | Telemaco  | Headless Chrome |
|--------------|-----------|-----------------|
| Memory       | 30 MB     | 200+ MB         |
| Binary size  | 70 MB     | 300+ MB         |
| Page load    | 85 ms     | ~500 ms         |
| Startup      | Instant   | ~2s             |
| Anti-detect  | Built-in  | None            |
| Puppeteer / Playwright | Yes | Yes          |

Roughly 12x faster page loads and 6x less memory than headless Chrome on
framework pages, with the same CDP automation surface.

## Features

- **Native rendering**: CSS layout and paint, viewport and full-page
  screenshots, activity-driven CDP screencasting, and PDF export.
- **Stealth mode**: wreq/BoringSSL transport, per-session fingerprint
  randomization, consistent browser identity, and a tracker blocklist.
- **CDP compatible**: `telemaco serve` speaks the Chrome DevTools Protocol, so
  Puppeteer, Playwright, and chromiumoxide connect out of the box.
- **MCP server**: stateful browser automation tools for Claude Desktop, Cursor,
  and any MCP client, over stdio or HTTP.
- **Hardened by default**: SSRF guards, a V8 termination watchdog per page, and
  process-level hard deadlines, so one bad page can never hang a worker.

## Images

Tagged by version (`albz222/telemaco:0.1.2`) as well as `latest`, built for
`linux/amd64` and `linux/arm64`.

## License

Apache License 2.0.
