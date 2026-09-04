# Getting started with Telemaco

A practical guide to building and using Telemaco. For the full documentation
see `README.md` and `docs/`.

Telemaco is a headless browser engine in Rust: it runs real JavaScript through
V8, keeps a real DOM, renders natively, speaks the Chrome DevTools Protocol,
and stands in for headless Chrome with Puppeteer and Playwright.

---

## 1. Getting the binaries

A build produces two binaries in `target/release/`:

| Binary | What it does |
|---|---|
| `telemaco` | CLI: `fetch`, `serve` (CDP server), `scrape`, `mcp` |
| `telemaco-worker` | Worker for parallel scraping; must sit next to `telemaco` for `scrape` |

Which features a binary has depends on how it was built. `--features render`
gives you screenshots and PDF; `--features render,stealth` adds TLS
impersonation on top. The `--stealth` flag works either way (consistent browser
fingerprint), but the transport-level impersonation needs the stealth build.
See §2.

The examples below assume the binary is on your `PATH`, or use a variable:

```bash
T=$PWD/target/release/telemaco
$T --version
```

Rather than setting that up by hand, the installer can do it for you.

### 1.1 Installer

`install.sh` downloads the right release archive for this machine and then
offers to register telemaco as an MCP server with the agents it finds:

```bash
./install.sh
```

It asks before touching anything, one agent at a time, and backs up every
config it edits. `--yes` accepts the defaults and registers nothing; `--prefix`
changes the destination (default `~/.local/bin`); `--from-source` builds
locally instead of downloading.

Claude Code goes through its own CLI (`claude mcp add`). Claude Desktop,
Cursor, and Windsurf are merged into their JSON config without losing other
servers; a file that does not parse is left alone rather than rewritten. Zed
only gets a printed snippet, because its `settings.json` accepts comments and
rewriting it as strict JSON would delete them.

Re-running it changes nothing once telemaco is registered.

### 1.2 Updating

```bash
telemaco update            # replace the binaries with the latest release
telemaco update --check    # report only; exit 1 means an update is available
```

The update keeps the build variant you installed, so a stealth install stays a
stealth install rather than quietly becoming the plain one. Both `telemaco` and
`telemaco-worker` are replaced together, since a mismatched pair breaks
`scrape`. The download is verified by running it before anything is replaced,
and the replacement itself is a rename, so a failure leaves the working install
untouched.

Run interactively, telemaco checks once a day whether a newer release exists
and says so. It never does this when stderr is not a terminal, which covers
scripts, CI, and MCP clients: a browser people reach for to avoid leaving
traces should not contact GitHub on its own. `TELEMACO_NO_UPDATE_CHECK=1`
switches the check off entirely.

`update` refuses when the binary it would replace turns out to be cargo build
output, which happens when `~/.local/bin/telemaco` is a symlink into a
checkout: replacing it would drop a downloaded binary into a build directory,
where the next `cargo build` silently reverts it. A directory named `target` is
only treated as build output when a `Cargo.toml` sits beside it.

Self-update is not available on Windows yet; a running `.exe` cannot be
replaced in place, so the command says so instead of half-finishing.

### 1.3 Docker

```bash
docker pull albz222/telemaco:latest
docker run -d -p 127.0.0.1:9222:9222 albz222/telemaco:latest
```

The container runs the CDP server on port 9222, so Puppeteer and Playwright
connect to `ws://127.0.0.1:9222` with nothing else to set up. Override the
entrypoint for a one-off command:

```bash
docker run --rm --entrypoint /telemaco albz222/telemaco:latest \
  fetch https://example.com --dump markdown
```

Images are built for `linux/amd64` and `linux/arm64`, tagged by version and
`latest`.

## 2. Building

The first build compiles V8 from source: around five minutes and a few GB of
disk. Later builds take seconds.

```bash
# Rendering (screenshots, PDF, screencast)
CARGO_INCREMENTAL=0 cargo build --release -p telemaco-cli --bins --features render

# Rendering plus full stealth (wreq/BoringSSL, needs cmake)
CARGO_INCREMENTAL=0 cargo build --release -p telemaco-cli --bins --features render,stealth
```

Requires Rust 1.75+ ([rustup.rs](https://rustup.rs)). Stealth also needs CMake
and Clang. Use `-p telemaco-cli` so cargo does not relink the whole workspace:
recompiling V8 is the cost worth avoiding.

## 3. First use: `fetch`

Loads a page, runs its JavaScript, and prints the result.

```bash
T=$PWD/target/release/telemaco

# Page title
$T fetch https://example.com --eval "document.title"

# Every link, one per line, as URL<TAB>text
$T fetch https://example.com --dump links

# Rendered HTML, after JavaScript has run
$T fetch https://news.ycombinator.com --dump html

# Markdown, which is the token-dense choice for an LLM
$T fetch https://example.com --dump markdown

# Text, waiting for dynamic content first
$T fetch https://example.com --wait-until networkidle0 --timeout 10 --dump text

# Write to a file
$T fetch https://example.com --dump text --output page.txt

# Raw HTTP body, binary-safe, bypassing the JS and DOM layers
$T fetch https://picsum.photos/200/300 --dump original > photo.jpg

# PNG screenshot, 1280x720 viewport by default
$T fetch https://example.com --screenshot page.png

# Wait for a selector to appear, then dump the page
$T fetch https://example.com --selector "h1" --dump text

# Override the User-Agent
$T fetch https://example.com --user-agent "TestAgent/1.0" --eval "navigator.userAgent"

# Cookies and storage that survive between runs
$T fetch https://example.com --storage-dir ~/.telemaco-store --dump cookies

# Every sub-resource the page references, one JSON object per line
$T fetch https://example.com --dump assets
```

`--dump` accepts: `assets`, `html`, `text`, `links`, `markdown`, `original`,
`cookies`.

A note on `--selector`: it does not narrow the output. It waits for a matching
element to appear, which is what you want for content built by JavaScript, and
then dumps the page as usual. With `--screenshot`, any `--eval` runs before the
capture, so you can scroll or prepare the page state first.

**Batch mode with `--file`:** URLs one per line, from a file or from stdin
(`-`); blank lines and `#` comments are ignored. Each URL is fetched raw
(`--dump original`) and prints a JSON status line:

```bash
$T fetch --file urls.txt --concurrency 5
cat urls.txt | $T fetch --file - --concurrency 5
# {"url":"...","ok":true,"status":200,"content_type":"text/html","bytes":446,"elapsed_ms":27}
```

For rendered or DOM output in batch, use `scrape` (§6). `--screenshot` is not
available in batch mode.

**Multi-statement JavaScript:** an `--eval` that starts with `const` and has
more than one statement returns `null`, because V8 gives `const` an empty
completion value. Wrap the snippet in an IIFE:

```bash
$T fetch https://example.com --eval "(function(){ const h = document.querySelector('h1'); return h ? h.textContent : null; })()"
```

**V8 flags:** `--v8-flags` is not global, so it goes before the subcommand.

```bash
$T --v8-flags "--expose-gc" fetch https://example.com --eval "typeof gc"   # -> function
```

## 4. Localhost and LAN sites

Telemaco blocks private addresses by default, as an SSRF guard. For local
testing pass `--allow-private-network`, or set
`TELEMACO_ALLOW_PRIVATE_NETWORK=1`:

```bash
$T fetch http://127.0.0.1:3000 --allow-private-network --dump text
$T serve --port 9222 --allow-private-network
```

Without the flag the error reads: `Access to private/internal IP address ... is
not allowed`.

## 5. CDP server: Puppeteer and Playwright

```bash
$T serve --port 9222
# endpoint: ws://127.0.0.1:9222

# Quick check, from another terminal:
curl -s http://127.0.0.1:9222/json/version
# {"Browser":"Chrome/145.0.0.0", ..., "webSocketDebuggerUrl":"ws://127.0.0.1:9222/devtools/browser"}
curl -s http://127.0.0.1:9222/json/list
```

Puppeteer:

```javascript
import puppeteer from 'puppeteer-core';
const browser = await puppeteer.connect({ browserWSEndpoint: 'ws://127.0.0.1:9222/devtools/browser' });
const page = await browser.newPage();
await page.goto('https://example.com');
console.log(await page.title());
await browser.disconnect();
```

Playwright:

```javascript
import { chromium } from 'playwright-core';
const browser = await chromium.connectOverCDP({ endpointURL: 'ws://127.0.0.1:9222' });
const page = await browser.newContext().then(ctx => ctx.newPage());
await page.goto('https://example.com');
console.log(await page.title());
```

Render-enabled builds also support `page.screenshot()`, full page included, and
`page.pdf()`.

Useful `serve` flags: `--host` (default `127.0.0.1`), `--workers <N>` (several
worker processes behind one port), `--max-connections` (default 128),
`--allow-file-access` (lets CDP clients navigate `file://`, off by default),
`--storage-dir`, `--quiet`.

## 6. Parallel scraping: `scrape`

```bash
# Run JavaScript across several URLs at once, as JSON
$T scrape https://example.com https://news.ycombinator.com \
  --concurrency 25 \
  --eval "document.querySelector('h1')?.textContent ?? document.title" \
  --format json
```

Typical output:

```json
{
  "total_urls": 2,
  "concurrency": 2,
  "total_time_ms": 1372,
  "avg_time_ms": 686.0,
  "results": [
    {
      "url": "https://example.com",
      "title": "Example Domain",
      "eval": "Example Domain",
      "time_ms": 686,
      "worker": 0
    }
  ]
}
```

`scrape` does **not** read stdin and does not accept `-`: pass every URL as an
argument. For raw batch input from stdin use `fetch --file -` (§3).

It needs `telemaco-worker` in the same directory as `telemaco`, or on `PATH`.

## 7. MCP, for Claude Desktop, Cursor, and others

```bash
$T mcp                      # stdio; the client launches the process
$T mcp --http --port 3000   # HTTP, endpoint http://127.0.0.1:3000/mcp
```

Run by hand, `telemaco mcp` looks like it hangs. It does not: a stdio server
waits silently for JSON-RPC on stdin. It prints a note explaining that when
stdin is a terminal.

Claude Desktop configuration:

```json
{
  "mcpServers": {
    "telemaco": { "command": "/absolute/path/to/telemaco", "args": ["mcp"] }
  }
}
```

Tools available, 37 as of 0.1.3: `browser_navigate`, `browser_snapshot`,
`browser_interactive_elements`, `browser_click`, `browser_fill`,
`browser_fill_form`, `browser_detect_forms`, `browser_type`,
`browser_press_key`, `browser_select_option`, `browser_evaluate`,
`browser_extract`, `browser_count`, `browser_get_attribute`, `browser_scroll`,
`browser_wait_for`, `browser_wait_for_text`, `browser_network_requests`,
`browser_console_messages`, `browser_get_cookies`, `browser_set_cookie`,
`browser_clear_cookies`, `browser_storage_state`, `browser_set_storage_state`,
`browser_tab_new`, `browser_tab_list`, `browser_tab_switch`,
`browser_tab_close`, `browser_back`, `browser_forward`, `browser_reload`,
`browser_markdown`, `browser_links`, `browser_search`, `browser_close`,
`browser_screenshot`, `browser_pdf`.

Quick MCP handshake check over stdio:

```bash
printf '%s\n%s\n%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"1.0"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
  | $T mcp 2>/dev/null | grep -c browser_
# -> 37
```

## 8. Global flags and environment variables

These work before or after the subcommand, on `fetch`, `serve`, `scrape`, and
`mcp`:

| Flag | Effect |
|---|---|
| `--proxy <URL>` | HTTP or SOCKS5 proxy, e.g. `socks5://user:pass@host:1080` |
| `--stealth` | Consistent browser fingerprint and tracker blocking; with a stealth build, TLS impersonation too |
| `--allow-private-network` | Permit loopback, RFC1918, and link-local addresses, for local development |
| `--obey-robots` | Respect robots.txt before navigating, on fetch and scrape |
| `--user-agent <UA>` | Override the User-Agent |
| `--storage-dir <DIR>` | Cookies and localStorage that survive between runs |
| `--config <PATH>` | TOML config file; must exist if given |
| `-v, --verbose` | Debug logging |
| `--v8-flags "<FLAGS>"` | Raw V8 flags; **not global**, so it goes before the subcommand |

Environment variables worth knowing, with the full list in
[docs/Environment-variables.md](docs/Environment-variables.md):

| Variable | Default | What it does |
|---|---|---|
| `TELEMACO_NAV_TIMEOUT_MS` | 30000 | Ceiling on a single navigation |
| `TELEMACO_SCRIPT_DEADLINE_MS` | 30000 | Budget for the page's script phase |
| `TELEMACO_ALLOW_PRIVATE_NETWORK` | off | Same as the SSRF flag |
| `TELEMACO_TIMEZONE` | Europe/Berlin | Pin the timezone, to match your proxy |
| `TELEMACO_PROXY` | — | Default proxy for `scrape` workers |
| `TELEMACO_MCP_MAX_CHARS` | 4000 | Characters per page from `browser_markdown` |

## 9. Testing

### 9.1 CLI smoke battery, offline, about a minute

End-to-end checks against a local fixture: no external network, no site to
depend on. Paste the whole block into a terminal; each check prints `PASS` or
`FAIL`.

```bash
T=$PWD/target/release/telemaco
D=$(mktemp -d); PORT=8099; CDP=9223; U="http://127.0.0.1:$PORT"
FAILED=0; ok(){ echo "PASS: $1"; }; bad(){ echo "FAIL: $1"; FAILED=1; }

# --- local fixture ---
mkdir -p "$D/fixture/private"
cat > "$D/fixture/index.html" <<'EOF'
<!DOCTYPE html>
<html><head><title>Telemaco fixture</title></head><body>
  <h1 id="heading">Fixture heading</h1><p>Static paragraph.</p>
  <a href="/page2.html">Page 2</a>
  <script>
    document.cookie = "tm_cookie=123; path=/";
    const d = document.createElement('p');
    d.id = 'dynamic';
    d.textContent = 'Injected by JS';
    document.body.appendChild(d);
  </script>
</body></html>
EOF
printf '<!DOCTYPE html><html><head><title>Fixture page 2</title></head><body>two</body></html>' > "$D/fixture/page2.html"
printf 'User-agent: *\nDisallow: /private/\n' > "$D/fixture/robots.txt"
printf '<!DOCTYPE html><html><body>secret</body></html>' > "$D/fixture/private/secret.html"
printf '%s\n# comment\n%s\n' "$U/index.html" "$U/page2.html" > "$D/urls.txt"
python3 -m http.server "$PORT" --directory "$D/fixture" >/dev/null 2>&1 &
HTTPD=$!; sleep 1

# 1) version
# Compared against the manifest rather than a fixed string: a test pinned to
# the exact number breaks on every release.
V=$(grep -m1 '^version' "$(git rev-parse --show-toplevel)/Cargo.toml" | cut -d'"' -f2)
[ "$($T --version)" = "telemaco $V" ] && ok "version" || bad "version"

# 2) SSRF gate: without the flag, fetching 127.0.0.1 must fail
$T fetch "$U/index.html" --dump text 2>&1 | grep -q "not allowed" \
  && ok "SSRF blocked without the flag" || bad "SSRF gate"

# 3) local fetch with the SSRF flag
$T fetch "$U/index.html" --allow-private-network -q --dump text | grep -q "Fixture heading" \
  && ok "fetch with --allow-private-network" || bad "local fetch"

# 4) --eval against content built by JavaScript, wrapped in an IIFE
$T fetch "$U/index.html" --allow-private-network -q \
  --eval "(function(){ return document.querySelector('#dynamic').textContent; })()" \
  | grep -q "Injected by JS" && ok "--eval on dynamic content" || bad "--eval"

# 5) --dump cookies
$T fetch "$U/index.html" --allow-private-network -q --dump cookies | grep -q "tm_cookie" \
  && ok "--dump cookies" || bad "cookies"

# 6) --screenshot: a non-empty 1280x720 PNG. `sips` is macOS-only; the file
#    goes inside fixture/ so check 7 can fetch it back over HTTP.
$T fetch "$U/index.html" --allow-private-network -q --screenshot "$D/fixture/shot.png" >/dev/null 2>&1
[ -s "$D/fixture/shot.png" ] \
  && [ "$(sips -g pixelWidth "$D/fixture/shot.png" | awk '/pixelWidth/{print $2}')" = "1280" ] \
  && ok "--screenshot 1280x720" || bad "screenshot"

# 7) --dump original is binary-safe: the hashes must match
$T fetch "$U/shot.png" --allow-private-network -q --dump original > "$D/copy.png" 2>/dev/null
[ "$(shasum "$D/fixture/shot.png" | awk '{print $1}')" = "$(shasum "$D/copy.png" | awk '{print $1}')" ] \
  && ok "--dump original is binary-safe" || bad "dump original"

# 8) batch mode from stdin: two URLs
N=$(cat "$D/urls.txt" | $T fetch --file - --allow-private-network 2>/dev/null | grep -c '"ok":true')
[ "$N" = "2" ] && ok "fetch --file with 2 URLs" || bad "batch mode"

# 9) parallel scrape
$T scrape "$U/index.html" "$U/page2.html" --allow-private-network --concurrency 2 \
  --eval "document.title" --format json 2>/dev/null | grep -q '"total_urls": 2' \
  && ok "parallel scrape" || bad "scrape"

# 10) --obey-robots blocks a Disallowed path
$T fetch "$U/private/secret.html" --allow-private-network --obey-robots 2>&1 \
  | grep -q "Blocked by robots.txt" && ok "--obey-robots" || bad "obey-robots"

# 11) --storage-dir: the cookie survives between runs
rm -rf "$D/store"
$T fetch "$U/index.html" --allow-private-network -q --storage-dir "$D/store" >/dev/null 2>&1
$T fetch "$U/page2.html" --allow-private-network -q --storage-dir "$D/store" \
  --eval "document.cookie" | grep -q "tm_cookie=123" \
  && ok "--storage-dir persists cookies" || bad "storage-dir"

# 12) CDP server
$T serve --port "$CDP" --allow-private-network >/dev/null 2>&1 &
SVR=$!; sleep 2
curl -s "http://127.0.0.1:$CDP/json/version" | grep -q "webSocketDebuggerUrl" \
  && ok "serve CDP" || bad "serve"

# 13) MCP: stdio handshake and tool list
printf '%s\n%s\n%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"1.0"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
  | $T mcp 2>/dev/null | grep -q '"browser_navigate"' \
  && ok "MCP tools/list" || bad "MCP"

kill $HTTPD $SVR 2>/dev/null
[ "$FAILED" = "0" ] && echo "ALL CHECKS PASSED" || echo "THERE WERE FAILURES"
```

The battery binds fixed ports. If a stale server already holds one, the
requests reach the wrong process and most checks fail for a reason that has
nothing to do with the code, so check the port before trusting a red run.

### 9.2 Rust suite

**Use `cargo nextest`, not `cargo test`**: the engine holds a single V8 isolate
per process, so the runtime tests fail under `cargo test`. Nextest runs each
test in its own process.

```bash
cargo install cargo-nextest   # if missing

# Everything
cargo nextest run --release --features render --no-fail-fast

# One crate
cargo nextest run --release --features render -p telemaco-cli
```

### 9.3 Acceptance suite

The behavioral gate is the suite in `acceptance/`: 41 stages that must stay at
41/41.

```bash
TELEMACO_BIN=./target/release/telemaco python3 acceptance/run.py
```

It serves its own fixtures on a port picked at runtime, so it is deterministic,
offline, and cannot collide with a server already running. Details, and how to
add a stage, are in [acceptance/README.md](acceptance/README.md).

## 10. Common problems

| Symptom | Cause and fix |
|---|---|
| `--eval` returns `null` | A multi-statement snippet starting with `const`: wrap it in an IIFE (§3) |
| Fetching `localhost` is refused | The SSRF gate: add `--allow-private-network` (§4) |
| `--screenshot` unavailable | Needs a build with `--features render` (§2) |
| Stealth looks weak | The binary was built without the `stealth` feature: rebuild with `--features render,stealth`, which needs cmake |
| `telemaco mcp` seems to hang | It does not: a stdio server waits for JSON-RPC on stdin. Launch it from an MCP client, or pipe a request in (§7) |
| `scrape` will not start | `telemaco-worker` is missing next to the `telemaco` binary |
| `scrape -` or piping into `scrape` fails | `scrape` does not read stdin: pass URLs as arguments, or use `fetch --file -` (§3) |
| `--v8-flags` rejected after the subcommand | It is not global: put it first, e.g. `$T --v8-flags "..." fetch ...` (§3) |
| `--selector` does not seem to filter | Working as intended: it waits for the element, then dumps the whole page (§3) |
| The first build takes forever | Expected: it compiles V8, around five minutes. Later builds take seconds; use `-p` to narrow the scope |
