## `telemaco`

Top-level flags apply to every subcommand.

```
-v, --verbose                Enable info logging
-p, --port <PORT>            CDP port (default 9222)
    --proxy <URL>            HTTP or SOCKS5 proxy
    --stealth                Consistent browser fingerprint + tracker blocking
    --obey-robots            Respect robots.txt
    --user-agent <UA>        Override the User-Agent
    --storage-dir <DIR>      Persistent cookies and localStorage
    --allow-private-network  Permit loopback / RFC1918 / link-local
    --config <PATH>          TOML config file (must exist if given)
    --v8-flags <FLAGS>       Raw V8 flags, applied at startup
-h, --help                   Help
-V, --version                Version
```

## `telemaco fetch <URL>`

Load a URL and print its content or an evaluated expression.

```
    --dump <FORMAT>          html | text | links | markdown | original | assets | cookies
                             (default html)
    --selector <CSS>         Narrow output to a CSS selector
    --wait <SECONDS>         Fixed post-load delay; omitted uses adaptive settle (5s cap)
    --timeout <SECONDS>      Navigation timeout (default 30)
    --wait-until <LEVEL>     domcontentloaded | load | networkidle2 | networkidle0
                             (default load)
    --user-agent <UA>        Override the User-Agent
    --proxy <URL>            HTTP or SOCKS5 proxy
    --stealth                Consistent browser fingerprint + tracker blocking (global)
-e, --eval <JS>              Evaluate JS, print the result as JSON
-o, --output <FILE>          Write to a file instead of stdout
-s, --screenshot <FILE>      Capture the settled page as PNG (single URL)
-q, --quiet                  Suppress info logging
-v, --verbose                Enable verbose logging
```

`--screenshot` requires a render-enabled build. It uses a 1280×720 viewport by
default and may be combined with `--eval`; the expression runs before capture,
which is useful for scrolling or preparing page state. It is not available in
`--file` batch mode.

When `--wait` is omitted, Telemaco drives timers and async work until the page
becomes quiescent, with a five-second ceiling. Supplying `--wait N` instead
requests a fixed `N`-second delay. `--timeout` separately bounds navigation.

`--dump` values:

| Value      | Output                                                    |
| ---------- | --------------------------------------------------------- |
| `html`     | Rendered HTML (default)                                   |
| `text`     | Plain text                                                |
| `markdown` | Markdown conversion                                       |
| `links`    | Every `<a href>`, one URL per line                        |
| `assets`   | Every external resource, one JSON object per line (DOM assets plus `fetch()`/XHR requests) |
| `original` | Raw HTTP response body (binary-safe, bypasses the engine) |
| `cookies`  | All cookies in the jar as a JSON array, including HttpOnly cookies invisible to `document.cookie` |

## `telemaco serve`

Run the CDP server. Puppeteer and Playwright connect over WebSocket.

```
-p, --port <PORT>            CDP port (default 9222)
    --host <HOST>            Bind host (default 127.0.0.1)
    --proxy <URL>            HTTP or SOCKS5 proxy
    --user-agent <UA>        Override the User-Agent
    --stealth                Consistent browser fingerprint + tracker blocking (global)
    --workers <N>            Worker processes (default 1)
    --allow-file-access      Permit CDP clients to navigate to file:// URLs
    --storage-dir <DIR>      Persistent cookies and localStorage
    --allow-private-network  Permit loopback / RFC1918 / link-local
-q, --quiet                  Suppress info logging
-v, --verbose                Enable info logging
```

Default endpoint is `ws://127.0.0.1:9222`.

## `telemaco scrape [URLS]...`

Run a JS expression across many URLs in parallel.

```
-e, --eval <JS>              JS to run on each page
    --concurrency <N>        Parallel pages (default 10)
    --format <FORMAT>        Output format (default json)
    --timeout <SECONDS>      Per-URL timeout (default 60)
    --proxy <URL>            HTTP or SOCKS5 proxy
    --stealth                Consistent browser fingerprint + tracker blocking (global)
    --allow-private-network  Permit loopback / RFC1918 / link-local
-q, --quiet                  Suppress info logging
-v, --verbose                Enable verbose logging
```

`--stealth`, `--proxy`, and `--allow-private-network` are global flags: they work before or after any subcommand, so each worker in a `scrape` run inherits stealth too.

Read URLs from stdin with `-`:

```bash
cat urls.txt | telemaco scrape - --eval "document.title" --concurrency 20
```

Requires `telemaco-worker` next to `telemaco` in `PATH`.

## `telemaco mcp`

Run telemaco as an MCP server.

```
    --http                   HTTP transport instead of stdio
    --host <HOST>            HTTP bind host (default 127.0.0.1)
    --port <PORT>            HTTP port (default 3000)
    --proxy <URL>            HTTP or SOCKS5 proxy
    --user-agent <UA>        Override the User-Agent
    --stealth                Consistent browser fingerprint + tracker blocking (global)
    --allow-private-network  Permit loopback / RFC1918 / link-local
    --max-chars <N>          Cap on extracted page text (0 = unlimited)
-v, --verbose                Enable info logging
```

`--max-chars` overrides the configured cap for `browser_markdown` and `browser_snapshot`; a `max_chars` argument on an individual tool call still wins over it. Every other extraction cap is set through the config file or the environment: see [Configuration file](Configuration-file.md).

`--host` only applies with `--http`. The default `127.0.0.1` keeps the server loopback-only; set `0.0.0.0` to bind all interfaces (for example a Docker Compose sidecar) and pair it with `TELEMACO_MCP_ALLOWED_ORIGINS`.

Default transport is stdio. See [Use the MCP server](Use-the-MCP-server.md).

Render-enabled builds add `browser_screenshot` and `browser_pdf` to the MCP
tool list. Streaming screencasts are available through CDP rather than MCP.
