## Docker

```bash
docker run -d \
  --name telemaco \
  --restart unless-stopped \
  -p 127.0.0.1:9222:9222 \
  -v /srv/telemaco/data:/data \
  AlbertoBarrago/telemaco \
  serve --host 0.0.0.0 --storage-dir /data --stealth
```

The image runs `telemaco serve` by default. Override with arguments after the image name.

## Systemd

`/etc/systemd/system/telemaco.service`:

```ini
[Unit]
Description=Telemaco headless browser
After=network.target

[Service]
ExecStart=/usr/local/bin/telemaco serve --port 9222 --stealth --storage-dir /var/lib/telemaco
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

## Workers

`telemaco serve --workers N` runs N CDP server workers behind the listener.

```bash
telemaco serve --workers 4
```

Use one worker per CPU core. Each worker handles its own pool of pages. Sessions are sticky to a worker.

## V8 heap

Default V8 heap is 4 GB on 64-bit systems. The defaults also cap the young generation (`--max-semi-space-size=4`) and pass `--optimize-for-size` to hold RSS down. Override:

```bash
telemaco serve --v8-flags "--max-old-space-size=2048"
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

Per-container with Docker:

```bash
docker run --memory=4g --cpus=2 ...
```

## Reverse proxy

Expose telemaco on TLS through nginx or caddy:

```nginx
location /telemaco/ {
  proxy_pass http://127.0.0.1:9222/;
  proxy_http_version 1.1;
  proxy_set_header Upgrade $http_upgrade;
  proxy_set_header Connection "upgrade";
  proxy_read_timeout 86400;
}
```

CDP needs WebSocket upgrade and long read timeouts.

## Authentication

Telemaco's CDP server has no built-in auth. Anyone who can reach the port can drive the browser. Options:

- Bind to `127.0.0.1` and require SSH for access (default).
- Put it behind a reverse proxy that enforces auth.
- Use Docker network isolation.

Never bind `0.0.0.0` on a public IP without one of the above.

## MCP HTTP transport

`telemaco mcp --http` binds `127.0.0.1` by default. To reach it from another container, bind with `--host 0.0.0.0` and set an `Origin` allowlist so a browser page cannot drive it cross-origin:

```bash
TELEMACO_MCP_ALLOWED_ORIGINS="https://app.example.com" \
  telemaco mcp --http --host 0.0.0.0 --port 3000
```

Request bodies are capped at 16 MiB. Like the CDP server it has no built-in auth, so keep it on an internal network or behind an authenticating proxy. See [Use the MCP server](Use-the-MCP-server.md).

## Observability

```bash
telemaco serve --verbose
RUST_LOG=telemaco=debug telemaco serve
```

`--verbose` enables info-level logs. `RUST_LOG=telemaco=debug` enables debug-level. Logs go to stderr.

## Reliability and timeouts

The engine is hardened so one page cannot hang, crash, or wedge a worker. A V8 watchdog terminates runaway scripts and microtask storms, DOM ops are panic-safe, cyclic DOM mutations are rejected, and the CDP server terminates any single command that overruns its budget so a hung session cannot stall the others. Scripted `fetch()`/XHR and navigation are timeout-bounded. You can point the server at arbitrary or heavy pages without a stuck worker.

Tune the bounds with environment variables (see [Environment variables](Environment-variables.md)):

```bash
TELEMACO_NAV_TIMEOUT_MS=60000 \
TELEMACO_SCRIPT_DEADLINE_MS=45000 \
TELEMACO_MODULE_BUDGET_MS=10000 \
TELEMACO_CDP_COMMAND_TIMEOUT_MS=70000 \
TELEMACO_FETCH_TIMEOUT_MS=20000 \
  telemaco serve
```

`TELEMACO_NAV_TIMEOUT_MS` is the per-navigation ceiling (default 30000). `TELEMACO_SCRIPT_DEADLINE_MS` bounds the complete script phase (default 30000), while `TELEMACO_MODULE_BUDGET_MS` bounds each enhancement module's graph loading and evaluation (default 3000; unmounted SPA shells use the full script deadline). `TELEMACO_CDP_COMMAND_TIMEOUT_MS` is the outer per-CDP-command V8 deadline (default 60000, `0` disables), so keep it above the navigation ceiling. `TELEMACO_FETCH_TIMEOUT_MS` bounds scripted fetch/XHR and module network requests (default 30000).
