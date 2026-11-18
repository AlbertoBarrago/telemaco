## Systemd

Run the MCP server over HTTP as a service. `/etc/systemd/system/telemaco.service`:

```ini
[Unit]
Description=Telemaco MCP server
After=network.target

[Service]
ExecStart=/usr/local/bin/telemaco --stealth mcp --http --port 3000
Restart=always
RestartSec=5
User=telemaco
Group=telemaco
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
```

```bash
systemctl enable --now telemaco
journalctl -fu telemaco
```

## V8 heap

Default V8 heap is 4 GB on 64-bit systems. The defaults also cap the young generation (`--max-semi-space-size=4`) and pass `--optimize-for-size` to hold RSS down. Override:

```bash
telemaco --v8-flags "--max-old-space-size=2048" mcp --http
```

Flags you pass are appended after the defaults, and V8 uses the last value for a repeated flag, so your `--max-old-space-size` wins while the memory-tuning defaults stay in effect. Lower for memory-constrained hosts, raise for heavy SPAs.

## Parallel scrape

`telemaco scrape` fans out a list of URLs across worker processes:

```bash
telemaco scrape \
  --concurrency 20 \
  --format json \
  --timeout 60 \
  url1 url2 url3 ...
```

Reads URLs from stdin:

```bash
cat urls.txt | telemaco scrape --concurrency 20 -
```

Requires `telemaco-worker` next to `telemaco` in `PATH`.

## Resource limits

Per-process memory cap with systemd:

```ini
[Service]
MemoryMax=4G
MemoryHigh=3G
```

## MCP HTTP transport

`telemaco mcp --http` binds `127.0.0.1` by default. To reach it from another host or container, bind with `--host 0.0.0.0` and set an `Origin` allowlist so a browser page cannot drive it cross-origin:

```bash
TELEMACO_MCP_ALLOWED_ORIGINS="https://app.example.com" \
  telemaco mcp --http --host 0.0.0.0 --port 3000
```

Request bodies are capped at 16 MiB. It has no built-in auth: anyone who can reach the port can drive the browser. Keep it on `127.0.0.1` (and use SSH for remote access), on an internal network, or behind a reverse proxy that enforces auth. Never bind `0.0.0.0` on a public IP without one of these. See [Use the MCP server](Use-the-MCP-server.md).

## Observability

```bash
telemaco --verbose mcp --http
RUST_LOG=telemaco=debug telemaco mcp --http
```

`--verbose` enables info-level logs. `RUST_LOG=telemaco=debug` enables debug-level. Logs go to stderr.

## Reliability and timeouts

The engine is hardened so one page cannot hang, crash, or wedge a worker. A V8 watchdog terminates runaway scripts and microtask storms, DOM ops are panic-safe, and cyclic DOM mutations are rejected. Scripted `fetch()`/XHR and navigation are timeout-bounded. You can point Telemaco at arbitrary or heavy pages without a stuck worker.

Tune the bounds with environment variables (see [Environment variables](Environment-variables.md)):

```bash
TELEMACO_NAV_TIMEOUT_MS=60000 \
TELEMACO_SCRIPT_DEADLINE_MS=45000 \
TELEMACO_MODULE_BUDGET_MS=10000 \
TELEMACO_FETCH_TIMEOUT_MS=20000 \
  telemaco mcp --http
```

`TELEMACO_NAV_TIMEOUT_MS` is the per-navigation ceiling (default 30000). `TELEMACO_SCRIPT_DEADLINE_MS` bounds the complete script phase (default 30000, capped by the navigation timeout), while `TELEMACO_MODULE_BUDGET_MS` bounds each enhancement module's graph loading and evaluation (default 3000; unmounted SPA shells use the full script deadline). `TELEMACO_FETCH_TIMEOUT_MS` bounds scripted fetch/XHR and module network requests (default 30000).
