<div align="center">
  <img src="assets/icon.png?v=2" alt="Telemaco" width="80" />

  <h1>Telemaco</h1>

  <p>
    <strong>Control the web with precision.</strong><br>
    A headless browser engine in Rust, built for web scraping and AI agents.<br>
    Real JavaScript, real DOM, native layout and paint. No Chromium required.
  </p>

  <p>
    <a href="https://github.com/AlbertoBarrago/telemaco/releases"><img src="https://img.shields.io/github/v/release/AlbertoBarrago/telemaco?style=for-the-badge&label=release&color=45B1F8" alt="Release"></a>
    <a href="https://github.com/AlbertoBarrago/telemaco/stargazers"><img src="https://img.shields.io/github/stars/AlbertoBarrago/telemaco?style=for-the-badge&label=stars&color=34D399" alt="GitHub stars"></a>
    <a href="https://github.com/AlbertoBarrago/telemaco/blob/main/LICENSE"><img src="https://img.shields.io/github/license/AlbertoBarrago/telemaco?style=for-the-badge&label=license&color=9AA7B8" alt="License"></a>
    <a href="https://github.com/AlbertoBarrago/telemaco/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/AlbertoBarrago/telemaco/ci.yml?style=for-the-badge&label=CI&color=45B1F8" alt="CI"></a>
    <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/rust-1.85%2B-45B1F8?style=for-the-badge&logo=rust&logoColor=white" alt="Rust"></a>
    <a href="https://github.com/AlbertoBarrago/telemaco"><img src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux-9AA7B8?style=for-the-badge" alt="Platforms"></a>
  </p>
</div>

---

Telemaco runs real JavaScript through V8 (`deno_core`), keeps a real DOM tree,
and owns its layout and paint pipeline. It has three surfaces:

- `telemaco fetch`: load a page from the shell and print HTML, text,
  Markdown, links, assets, cookies, an evaluated expression or a screenshot.
- `telemaco mcp`: an MCP server that gives AI agents stateful browser tools,
  plus `telemaco install` to wire it into coding agents.
- The `telemaco` crate: the engine as a Rust library.

Telemaco is a derivative work of
[Obscura](https://github.com/h4ckf0r0day/obscura), licensed under the Apache
License, Version 2.0. See [`NOTICE`](NOTICE) for the full attribution.

### Telemaco and Obscura

Telemaco forked Obscura at the end of August 2026 (Obscura `14da23d`,
imported as Telemaco's first commit `a7657a6`) and renamed the crates, the
binary and the project identity. Since then the two ship different things.
Telemaco dropped the CDP server, `serve`, `scrape`, the worker binary, the
Docker image and Windows builds (see [Removed](#removed)), and added the
agent installer (`telemaco install`), `telemaco update`, `fetch --focus`,
paginated `browser_markdown`, configurable MCP extraction limits, a DOM node
budget, and a 44-stage acceptance suite.

The engine underneath is still largely Obscura's code. Of the 150,478
non-blank lines of Rust and JavaScript in Obscura's `crates/` at the fork
point, 132,622 (88%) are still present, unchanged, in the same files on
`main`; the removed CDP server accounts for most of the rest. Method: a
per-file line diff of each Obscura source file against its Telemaco
counterpart, after the `obscura` to `telemaco` rename.

## Highlights

- **Native rendering**: CSS layout and paint, viewport and full-page
  screenshots, and PDF export, without Chromium or a WebView.
- **Stealth mode**: wreq/BoringSSL transport, per-session fingerprint
  randomization, consistent browser identity, and a tracker blocklist.
  Rendering stays available when stealth is on.
- **MCP server**: stateful browser automation tools for Claude Desktop,
  Cursor and any MCP client, over stdio or HTTP.
- **Hardened by default**: SSRF guard on loopback, RFC1918 and link-local
  targets; a V8 termination watchdog per page; a per-document DOM node
  budget; a process-level hard deadline for `fetch`.

## Install

### install.sh (recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/AlbertoBarrago/telemaco/main/install.sh | bash
```

Installs the prebuilt binary for your platform (or builds from source if none
is published), adds it to your `PATH`, then offers to run `telemaco install`
to configure your AI coding agents. Options: `--prefix <dir>`,
`--from-source`, `--yes`, `--uninstall` (removes the binary only; run
`telemaco uninstall` first to also remove agent configs). See
[Agent setup](#agent-setup-telemaco-install) below.

### Homebrew (macOS)

```bash
brew tap albertobarrago/telemaco
brew install telemaco
```

Installs the render-enabled build as `telemaco`. The stealth variant is a
separate formula that installs as `telemaco-stealth`, so the two can coexist:

```bash
brew install telemaco-stealth
telemaco-stealth --stealth fetch https://example.com --dump text
```

Bottles are published for Apple Silicon and Intel macOS. On Linux use
`install.sh` or a prebuilt archive. Upgrade with `brew upgrade` rather than
`telemaco update`, which would replace a binary Homebrew owns.

### Updating

```bash
telemaco update            # replace the binary with the latest release
telemaco update --check    # report only; exit 1 means an update is available
telemaco update --force    # reinstall even when already on the latest version
```

The update keeps the build variant you installed, so a stealth install stays
a stealth install. The download is verified by running it before anything is
replaced, and the replacement is a rename, so a failure leaves the working
install untouched.

Run interactively, telemaco checks once a day whether a newer release exists
and says so. It never does this when stderr is not a terminal, which covers
scripts, CI and MCP clients. `TELEMACO_NO_UPDATE_CHECK=1` switches the check
off entirely.

`update` refuses when the binary it would replace is cargo build output (for
example when `~/.local/bin/telemaco` is a symlink into a checkout), because the
next `cargo build` would silently revert it. A directory named `target` is
only treated as build output when a `Cargo.toml` sits beside it.

### Prebuilt binaries

Grab the latest archive from
[Releases](https://github.com/AlbertoBarrago/telemaco/releases). Archives are
built for `x86_64-linux`, `aarch64-linux`, `aarch64-macos` and
`x86_64-macos`:

```bash
curl -LO https://github.com/AlbertoBarrago/telemaco/releases/latest/download/telemaco-x86_64-linux.tar.gz
tar xzf telemaco-x86_64-linux.tar.gz
./telemaco fetch https://example.com --eval "document.title"
```

Each archive contains the single `telemaco` binary.

| Archive suffix        | Rendering | Stealth transport |
|-----------------------|-----------|-------------------|
| none                  | Yes       | No                |
| `-stealth`            | Yes       | Yes               |
| `-no-render`          | No        | No                |
| `-no-render-stealth`  | No        | Yes               |

There are no Windows release archives; build from source there.

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

Requires Rust 1.85+ ([rustup.rs](https://rustup.rs)). The first build
compiles V8 from source: about 5 minutes and a few GB of disk, cached
afterwards. The stealth build also compiles BoringSSL, which needs CMake and
Clang. On Ubuntu/Debian:

```bash
sudo apt-get install build-essential cmake clang libclang-dev llvm-dev
```

### Crates.io

The crates are published to [crates.io](https://crates.io), so you can use
Telemaco as a library or build the CLI from the registry:

```bash
cargo add telemaco                          # the engine, driven from Rust
cargo install telemaco-cli --features render # the CLI
```

The publishable set: `telemaco` (embeddable API), `telemaco-cli`, and the
workspace crates `telemaco-dom`, `telemaco-net`, `telemaco-js`,
`telemaco-browser`, `telemaco-mcp`, `telemaco-render`.

Layout and text come from two Telemaco-maintained forks, published under the
upstream lib names (`taffy` / `cosmic_text`) at exactly the version Telemaco
builds against:

- [`mentore`](https://crates.io/crates/mentore): the layout engine, a fork of
  Taffy.
- [`athena-text`](https://crates.io/crates/athena-text): the text engine, a
  fork of cosmic-text (including the CSS line-breaking work).

The crates that embed V8 (`telemaco`, `telemaco-js`, `telemaco-browser`,
`telemaco-mcp`) cannot be rendered by docs.rs, because building them requires
compiling V8. Generate their API docs locally with `cargo doc --no-deps --open`.

## Quick start

### Fetch a page

```bash
# Page title
telemaco fetch https://example.com --eval "document.title"

# Rendered HTML (the default dump)
telemaco fetch https://news.ycombinator.com

# Readable text or Markdown
telemaco fetch https://example.com --dump text
telemaco fetch https://example.com --dump markdown

# Only the Markdown blocks around some keywords
telemaco fetch https://example.com --dump markdown --focus pricing --focus plans

# All links
telemaco fetch https://example.com --dump links

# Every sub-resource URL the page references (NDJSON)
telemaco fetch https://example.com --dump assets

# Raw response body, binary-safe, bypassing the JS/DOM layer
telemaco fetch https://picsum.photos/200/300 --dump original > photo.jpg

# Write output to a file
telemaco fetch https://example.com --dump text --output page.txt

# Through an HTTP or SOCKS proxy
telemaco --proxy socks5://127.0.0.1:1080 fetch https://example.com --dump text

# Wait for dynamic content, bound navigation time
telemaco fetch https://example.com --wait-until networkidle0 --timeout 10

# Capture the settled page as PNG (render-enabled build)
telemaco fetch https://example.com --screenshot page.png
```

### Many URLs

`--file` reads newline-delimited URLs (`-` for stdin) and fetches each raw,
printing one JSON status line per URL. Batch mode only supports
`--dump original`; for rendered output run one `fetch` per URL.

```bash
telemaco fetch --file urls.txt --concurrency 8 --output results.ndjson
```

### Localhost and LAN dev servers

Fetches to private and internal IPs are blocked by default (SSRF protection).
For local testing pass `--allow-private-network` or set
`TELEMACO_ALLOW_PRIVATE_NETWORK=1`:

```bash
telemaco fetch http://127.0.0.1:3000 --allow-private-network --dump text
telemaco mcp --allow-private-network
```

The full allow/deny rules (including DNS-resolution-time checks) are in
[docs/Environment-variables.md](docs/Environment-variables.md).

## MCP (Model Context Protocol)

Telemaco ships an MCP server that exposes browser automation tools to AI
agents (Claude Desktop, Cursor and other MCP clients).

```bash
telemaco mcp                      # stdio, for clients that launch a subprocess
telemaco mcp --http --port 8080   # HTTP, endpoint: http://127.0.0.1:8080/mcp
```

`--http` binds `127.0.0.1:3000` unless `--host`/`--port` say otherwise, and
has no built-in auth. `--max-chars N` overrides the character cap for
`browser_markdown` and `browser_snapshot` (`0` means unlimited).

The server answers `initialize` with plain capabilities. Add
`--agent-directives` to have it also tell the agent, on connect, that
Telemaco is how it should reach the web; `telemaco install` adds this flag
when you accept it, so a server you wire up by hand stays neutral.

Claude Desktop config:

```json
{
  "mcpServers": {
    "telemaco": { "command": "telemaco", "args": ["mcp"] }
  }
}
```

Tools:

- Navigation and tabs: `browser_navigate`, `browser_back`, `browser_forward`,
  `browser_reload`, `browser_close`, `browser_tab_new`, `browser_tab_list`,
  `browser_tab_switch`, `browser_tab_close`
- Reading: `browser_snapshot`, `browser_markdown`, `browser_links`,
  `browser_extract`, `browser_interactive_elements`, `browser_detect_forms`,
  `browser_get_attribute`, `browser_count`, `browser_search`
- Interaction: `browser_click`, `browser_fill`, `browser_fill_form`,
  `browser_type`, `browser_press_key`, `browser_select_option`,
  `browser_scroll`
- Waiting and scripting: `browser_wait_for`, `browser_wait_for_text`,
  `browser_evaluate`
- State: `browser_get_cookies`, `browser_set_cookie`, `browser_clear_cookies`,
  `browser_storage_state`, `browser_set_storage_state`
- Diagnostics: `browser_network_requests`, `browser_console_messages`
- Render-enabled builds only: `browser_screenshot`, `browser_pdf`

See [docs/Use-the-MCP-server.md](docs/Use-the-MCP-server.md) for arguments.

## Agent setup (`telemaco install`)

`telemaco install` wires Telemaco into the coding agents on the machine: the
MCP server, an instructions block in the agent's memory file, and a prompt hook
that reminds the model to use Telemaco when a prompt looks web-bound.

Run interactively it asks before each part that changes how the agent already
behaves: the prompt hook, whether the MCP server should tell the agent to reach
the web through Telemaco, auto-approving Telemaco's tools, and refusing the
agent's own web search. `--yes` accepts them all; the flags below decline them
one at a time.

```bash
telemaco install                                     # interactive, detects installed agents
telemaco install --yes                               # non-interactive, accepts the defaults
telemaco install --folder ./my-project               # configure one project directory
telemaco install --folder ~/alt-home -l global       # use that folder as home for a global install
telemaco install --target claude,cursor              # pick agents explicitly
telemaco install --dry-run                           # show what would change, write nothing
telemaco uninstall                                   # remove everything a global install added
telemaco uninstall --folder ./my-project             # remove it from one project directory
```

Supported: Claude Code, Cursor, OpenAI Codex CLI, Gemini CLI, Google
Antigravity, Codeium Windsurf, OpenCode, Roo Code / Cline, Pi Coding Agent,
DeepSeek Harness, Qwen Code, Factory Droid, Poolside Agent, Kiro, Hermes
Agent.

| Flag | Description |
|------|-------------|
| `-t`, `--target` | Comma-separated agent ids, or `auto`, `all`, `none` |
| `-l`, `--location` | `global` (default) or `local` |
| `-f`, `--folder` | A project directory, or the home directory for a global install rooted elsewhere; asks which one when `--location` does not say |
| `-y`, `--yes` | Non-interactive, accept defaults |
| `--stealth` | Put `--stealth` in the agent's MCP command |
| `--no-permissions` | Do not auto-approve Telemaco tools |
| `--no-block-web` | Leave the agent's own web search/fetch enabled |
| `--no-prompt-hook` | Skip the prompt hook; the MCP server and instructions block still install |
| `--dry-run` | Report the plan without writing |
| `--print-config <agent>` | Print the MCP snippet for one agent and exit |

`telemaco uninstall` takes the same `-t`/`-l`/`-f`/`-y`/`--dry-run` flags and
removes Telemaco from the agent configs instead of adding it.

By default the installer adds a guard that refuses the agent's built-in web
search and fetch, so web work goes through Telemaco. Decline it interactively
or pass `--no-block-web`; re-running with that flag removes a guard installed
earlier. `--no-prompt-hook` works the same way, and takes the config file with
it when the hook was the only thing in it.

Every config file is copied to `<file>.telemaco-backup` before the first
rewrite. A config that is not valid JSON is reported and left untouched rather
than replaced; one that uses comments (`.vscode/mcp.json` and friends) is
rewritten as strict JSON, and the note says so.

## CLI reference

Subcommands: `fetch`, `mcp`, `install`, `uninstall`, `update`. Running
`telemaco` with no subcommand prints help.

### Global flags

Valid before or after the subcommand.

| Flag | Description |
|------|-------------|
| `--proxy <URL>` | HTTP or SOCKS5 proxy (`fetch`, `mcp`) |
| `--stealth` | Anti-detection mode (`fetch`, `mcp`) |
| `--allow-private-network` | Permit loopback, RFC1918 and link-local targets |
| `--obey-robots` | Respect robots.txt before navigating (`fetch`) |
| `--config <PATH>` | TOML config file; must exist when given |
| `-v`, `--verbose` | Info logging |
| `--v8-flags <FLAGS>` | Raw V8 flags, applied once at startup |

### `telemaco fetch <URL>`

| Flag | Default | Description |
|------|---------|-------------|
| `--dump` | `html` | `html`, `text`, `markdown`, `links`, `assets`, `original`, `cookies` |
| `-e`, `--eval` | | JS expression evaluated on the page |
| `--selector` | | Wait for a CSS selector before reading the page |
| `--wait-until` | `load` | `load`, `domcontentloaded`, `networkidle0`, `networkidle2` |
| `--timeout` | `30` | Maximum navigation time in seconds |
| `--wait` | adaptive, up to `5` | Post-load settling; an explicit value is a fixed delay in seconds |
| `--focus` | | Keep only Markdown blocks around this keyword (repeatable) |
| `--focus-context` | `1` | Blocks kept around each `--focus` hit |
| `-s`, `--screenshot` | | Write a PNG screenshot (render-enabled build, single URL) |
| `-o`, `--output` | | Write dump or eval output to a file |
| `--user-agent` | | Override the User-Agent |
| `--storage-dir` | | Directory for persistent cookies and storage |
| `--file` | | Batch mode: newline-delimited URLs, `-` for stdin |
| `--concurrency` | `1` | Concurrent fetches in batch mode |
| `-q`, `--quiet` | | Suppress info logging |

The full reference, including `mcp`, is in
[docs/CLI-reference.md](docs/CLI-reference.md).

## Stealth mode

Build with `--features render,stealth` (or install the `-stealth` archive or
`telemaco-stealth` formula), then enable it at runtime with the global
`--stealth` flag. Stealth adds the wreq/BoringSSL transport, per-session
fingerprint randomization (GPU, screen, canvas, audio, battery), realistic
`navigator.userAgentData` (high-entropy values), trusted dispatched events,
and a tracker blocklist. Screenshots, PDF and MCP keep working in the stealth
build.

## Architecture

| Crate | Role |
|-------|------|
| `telemaco-cli` | CLI: `fetch`, `mcp`, `install`, `uninstall`, `update` |
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

The behavioral gate is the acceptance suite in
[`acceptance/`](acceptance/README.md), 44 stages that must stay at 44/44:

```bash
TELEMACO_BIN=./target/release/telemaco python3 acceptance/run.py
```

It serves its own fixtures on a port picked at runtime, so it is deterministic,
offline, and cannot collide with a server already running. Rendering changes
also go through `render-repros/run.sh`.

## Removed

Removed after the 0.2.6 release, which is the last one to ship them. Telemaco
is used through `fetch`, the MCP server and the Rust library; these parts
served other uses and cost build and CI time.

| Removed | Replacement or reason |
|---------|-----------------------|
| CDP server and `telemaco serve` (Puppeteer/Playwright) | Use the MCP server or the `telemaco` crate (#25) |
| `--port`, `--user-agent`, `--storage-dir` as top-level flags | They configured the CDP server; `fetch` keeps its own `--user-agent` and `--storage-dir` (#25) |
| Docker image and its workflow | It packaged the CDP server; install the binary instead (#25) |
| `tools/live-view.mjs` | A CDP viewer, gone with the CDP server (#25) |
| `telemaco remote` and the `telemaco-remote` crate (managing another machine over Tailcat) | No replacement (#27) |
| Build without paint in `telemaco-render` | Paint is mandatory; use the `-no-render` CLI builds to skip rendering (#27) |
| Windows release archives | Build from source on Windows; `telemaco update` says so (#27) |
| Self-hosted rustdoc in `api/` and the Docs workflow | `cargo doc --no-deps --open` (#27) |
| `telemaco scrape` and the `telemaco-worker` binary | `fetch --file` for raw batch fetches, one `fetch` per URL for rendered output (#28) |
| `TELEMACO_PROXY`, `TELEMACO_STEALTH`, `TELEMACO_OBEY_ROBOTS` | No longer read; use `--proxy`, `--stealth`, `--obey-robots` (#28) |
| Engine APIs only the CDP server used (binding calls, runtime console and exception events, JS network events, `BrowserContext::allow_file_access`) | No remaining caller (#28) |
| `vendor/` copies of taffy and cosmic-text | Unused since the move to `mentore` and `athena-text` (#22) |

Effect on a render+stealth build (`CARGO_BUILD_JOBS=2`), measured in #25 to
#28:

| | before | after |
|---|---:|---:|
| clean build | 277s | 247s |
| incremental, after a change to `bootstrap.js` | 63s | 37s |
| incremental, after a change to `paint.rs` | 79s | 55s |
| incremental, after a change to `telemaco-mcp/src/lib.rs` | 32s | 20s |

## Proxies for production

For production scraping, residential or ISP IPs usually beat datacenter
addresses. We use [NodeMaven](https://go.nodemaven.com/telemacoagentaugust):
residential and ISP proxies, sticky sessions, and per-country targeting
through the proxy username. Route Telemaco through it with the global
`--proxy` flag. Discount codes: `TELEMACO35` (35% off mobile and residential),
`TELEMACO40` (40% off ISP and static).

## License

Telemaco is licensed under the [Apache License, Version 2.0](LICENSE). It is
a derivative work of [Obscura](https://github.com/h4ckf0r0day/obscura), also
Apache-2.0; [`NOTICE`](NOTICE) carries the attribution.
