## Runtime

### `TELEMACO_ALLOW_PRIVATE_NETWORK`

Allow fetches to loopback (`127.0.0.0/8`), RFC1918 (`10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`), and link-local (`169.254.0.0/16`, including the `169.254.169.254` cloud-metadata endpoint) addresses. The deny-set also covers the unspecified address (`0.0.0.0` / `::`), IPv6 unique-local (`fc00::/7`), and any IPv4-mapped form of the above. Off by default to block SSRF.

The guard validates at DNS-resolution time as well as on literal hosts, so a public hostname that resolves to a forbidden address is rejected at connect time (DNS-rebinding safe), not just hosts written as raw IPs.

Truthy values: `1`, `true`, `yes`, `on`.

```bash
TELEMACO_ALLOW_PRIVATE_NETWORK=1 telemaco fetch http://localhost:8080
```

Per-process equivalent: `--allow-private-network` on any subcommand.

### `TELEMACO_NAV_TIMEOUT_MS`

Hard ceiling on a single navigation. Default 30000 (30 seconds). Applies to `Page.navigate` and the CLI `fetch` command.

```bash
TELEMACO_NAV_TIMEOUT_MS=60000 telemaco serve
```

### `TELEMACO_NAV_CHAIN_LIMIT`

How many documents a navigation chain may load, the first navigation included. Default 10, which allows the requested document and nine navigations the page itself triggers via `location` assignments or form submissions. Raise the value for an endpoint that chains longer for good reasons, such as an SSO handover across several providers. The low default is what stops a page that resets `location` on every load.

A zero is raised to 1. This loads the requested document. If the page wants to chain further afterwards, the call reports an error, as at any other limit. A value the engine does not read as a number is replaced by the default. This also applies to a negative value and to a value with a trailing space.

The time budget is not tied to this limit. A longer chain usually also needs a higher `TELEMACO_NAV_TIMEOUT_MS`, because its default of 30 seconds applies to the whole chain and not to the individual document.

```bash
TELEMACO_NAV_CHAIN_LIMIT=20 telemaco serve
```

### `TELEMACO_SCRIPT_DEADLINE_MS`

Soft deadline for the complete page script-execution phase, including classic scripts and ES modules. Default 30000 (30 seconds). Raise it for a heavy SPA whose initial module is responsible for mounting an otherwise empty document. The engine also uses this value as a hard V8 watchdog budget, with a one-second grace period, so a synchronous script cannot run forever.

```bash
TELEMACO_SCRIPT_DEADLINE_MS=60000 telemaco serve
```

### `TELEMACO_MODULE_BUDGET_MS`

Per-module graph-loading and evaluation budget for modules that enhance an already-rendered page. Default 3000 (3 seconds). Raise it when a module such as the Vite HMR client legitimately needs longer to evaluate:

```bash
TELEMACO_MODULE_BUDGET_MS=10000 telemaco serve
```

This shorter budget applies when the document body already contains more than 50 descendant nodes, where modules are normally progressive enhancement and should not delay navigation indefinitely. For an unmounted SPA shell, Telemaco instead gives each module the full `TELEMACO_SCRIPT_DEADLINE_MS` budget so the app has time to mount. Module network requests remain independently bounded by `TELEMACO_FETCH_TIMEOUT_MS`.

### `TELEMACO_CDP_COMMAND_TIMEOUT_MS`

Per-command deadline for the CDP server. A hung page (a runaway `Runtime.evaluate`, a synchronous DOM op) is terminated after this budget so one bad session cannot hold the shared V8 lock and stall the others. Default 60000 (60 seconds); `0` disables it. Navigation self-bounds via `TELEMACO_NAV_TIMEOUT_MS` well under this.

```bash
TELEMACO_CDP_COMMAND_TIMEOUT_MS=30000 telemaco serve
```

### `TELEMACO_FETCH_TIMEOUT_MS`

Request timeout for scripted `fetch()`, `XMLHttpRequest`, and ES-module loads. Without it a request to a server that accepts the connection but never responds (including a CORS preflight) hangs forever and the XHR is stuck with no completion event. Default 30000 (30 seconds).

```bash
TELEMACO_FETCH_TIMEOUT_MS=15000 telemaco serve
```

### `TELEMACO_PROXY`

Default proxy URL used by `telemaco-worker` for the parallel `scrape` command when no `--proxy` flag is set.

```bash
TELEMACO_PROXY=http://proxy.example.com:8080 telemaco scrape - < urls.txt
```

## Stealth and identity

These tune the browser identity the engine presents so it stays internally consistent. See [Configure stealth and proxies](Configure-stealth-and-proxies.md) for the full picture.

### `TELEMACO_TIMEZONE`

Pins the process timezone before V8/ICU reads it, so `Date` (`getTimezoneOffset`, `toString`) and `Intl.DateTimeFormat` report one consistent zone. Default `Europe/Berlin`. Set it to match the exit IP's region.

```bash
TELEMACO_TIMEZONE=America/New_York telemaco serve
```

### `TELEMACO_GEOLOCATION`

Override the coordinates the `navigator.geolocation` shim reports, as `lat,lon`. Without it the shim reports a fixed default. Keep it consistent with `TELEMACO_TIMEZONE` and the proxy region.

```bash
TELEMACO_GEOLOCATION="40.7128,-74.0060" telemaco serve
```

### `TELEMACO_PROFILE`

Pin a specific browser profile from the built-in pool by index (`0`-based). Each profile keeps `navigator.platform`, `userAgentData`, the UA string, and the GPU renderer internally consistent. Without it a single stable profile is used.

```bash
TELEMACO_PROFILE=2 telemaco serve
```

### `TELEMACO_ROTATE_PROFILE`

Opt into picking a random profile per browser context instead of the stable default. Leave it off when you pin a TLS fingerprint, proxy region, or timezone, since a rotated profile would no longer match those.

```bash
TELEMACO_ROTATE_PROFILE=1 telemaco serve
```

## MCP

### `TELEMACO_MCP_ALLOWED_ORIGINS`

Comma-separated `Origin` allowlist for the HTTP MCP transport (`telemaco mcp --http`). Off by default, which keeps the permissive behavior. When set, a browser request whose `Origin` is not listed is refused with `403` before it can drive the server; native, non-browser MCP clients (which send no `Origin`) are always allowed. Use it to stop cross-origin pages from reaching a loopback MCP port.

```bash
TELEMACO_MCP_ALLOWED_ORIGINS="https://app.example.com" telemaco mcp --http --host 0.0.0.0
```

### MCP extraction limits

Every cap an MCP tool applies to its own output can be raised or removed. They exist because MCP output lands directly in an agent's context window, so an uncapped dump of a large page can burn a whole window in a single call: they are a context budget, not an arbitrary restriction.

All of them take a plain integer, and `0` means unlimited. An unparsable value is ignored and the layer below it stands, so a typo in a shell export cannot silently clamp output to nothing.

| Variable | Controls | Default |
|---|---|---|
| `TELEMACO_MCP_MAX_CHARS` | characters of page text from `browser_markdown` and `browser_snapshot` | 4000 |
| `TELEMACO_MCP_MAX_LINKS` | anchors from `browser_links` | 100 |
| `TELEMACO_MCP_MAX_INTERACTIVE` | elements from `browser_interactive_elements` | 100 |
| `TELEMACO_MCP_MAX_SEARCH_RESULTS` | matches from `browser_search` | 10 |
| `TELEMACO_MCP_SEARCH_CONTEXT_CHARS` | characters around each `browser_search` match | 80 |
| `TELEMACO_MCP_MAX_NETWORK_REQUESTS` | entries from `browser_network_requests` | 500 |
| `TELEMACO_MCP_MAX_CONSOLE_MESSAGES` | entries from `browser_console_messages` | 500 |
| `TELEMACO_MCP_MAX_FORMS` | forms from `browser_detect_forms` | 100 |

```bash
# Pull whole pages instead of the first 4 KB
TELEMACO_MCP_MAX_CHARS=0 telemaco mcp
```

These sit in the middle of a four-layer precedence chain:

```text
tool call argument > CLI flag > environment variable > config file > default
```

So an environment variable overrides the config file, and is in turn overridden by `--max-chars` and by a `max_chars` argument on an individual tool call. Per-process equivalent: `--max-chars`. See [Configuration file](Configuration-file.md) for the file layer.

## Logging

### `RUST_LOG`

Standard `tracing` filter. Common settings:

```bash
RUST_LOG=telemaco=info telemaco serve
RUST_LOG=telemaco=debug telemaco serve
RUST_LOG=telemaco_cdp=trace,telemaco_browser=debug telemaco serve
```

`--verbose` on the CLI is equivalent to `RUST_LOG=telemaco=info`.

## Build

### `OPENSSL_NO_VENDOR`

Forces `cargo build` to use the system OpenSSL instead of compiling the vendored copy. Set to `1` on hosts where the vendored OpenSSL fails (older VPS with AVX-512 issues).

```bash
OPENSSL_NO_VENDOR=1 cargo build --release --features render
```

## V8

V8 flags are passed via `--v8-flags`, not environment variables:

```bash
telemaco serve --v8-flags "--max-old-space-size=2048 --expose-gc"
```

Defaults are `--max-old-space-size=4096 --max-semi-space-size=4 --optimize-for-size` on 64-bit systems (a 4 GB old-space ceiling, a capped young generation, and codegen tuned for a smaller footprint to cut RSS). Anything you pass with `--v8-flags` is appended after these, and V8 uses the last value for a repeated flag, so your value wins for that flag while the other defaults stay in effect.

## HTTP proxy environment

Telemaco does not honor `HTTP_PROXY` / `HTTPS_PROXY` / `NO_PROXY`. Use `--proxy` or `TELEMACO_PROXY`.
