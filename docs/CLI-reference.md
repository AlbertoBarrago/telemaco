## `telemaco`

Top-level flags. `--proxy`, `--stealth` and `--allow-private-network` apply to `fetch` and `mcp`, `--obey-robots` to `fetch`. Every flag below except `--v8-flags` is global, so it can go before or after the subcommand; `--v8-flags` must come before it. Run with no subcommand to print help.

Subcommands: `fetch`, `mcp`, `install`, `uninstall`, `update`.

```
-v, --verbose                Enable info logging
    --proxy <URL>            HTTP or SOCKS5 proxy
    --stealth                Consistent browser fingerprint + tracker blocking
    --obey-robots            Respect robots.txt
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
-e, --eval <JS>              Evaluate JS, print the result as JSON
    --selector <CSS>         Wait for a CSS selector before reading the page
    --wait-until <LEVEL>     domcontentloaded | load | networkidle2 | networkidle0
                             (default load)
    --timeout <SECONDS>      Navigation timeout (default 30)
    --wait <SECONDS>         Fixed post-load delay; omitted uses adaptive settle (5s cap)
    --focus <KEYWORD>        Keep only Markdown blocks around this keyword (repeatable)
    --focus-context <N>      Blocks kept around each --focus hit (default 1)
-s, --screenshot <FILE>      Capture the settled page as PNG (single URL)
-o, --output <FILE>          Write to a file instead of stdout
    --user-agent <UA>        Override the User-Agent
    --storage-dir <DIR>      Directory for persistent cookies and storage
    --file <PATH>            Batch mode: newline-delimited URLs, `-` for stdin
    --concurrency <N>        Concurrent fetches in batch mode (default 1)
-q, --quiet                  Suppress info logging
```

`--selector` does not narrow the output. It waits until an element matching
the selector exists, driving the page's event loop meanwhile so content built
by JavaScript can appear, then dumps, evaluates or captures the whole page as
usual. The wait is bounded by `--wait` (5 seconds when omitted); if nothing
matches by then, a warning goes to stderr and the page is read anyway. To keep
only part of the output, use `--eval` with a DOM query, or `--focus` with
`--dump markdown`.

`--focus` applies to `--dump markdown`. It keeps each block that contains a
keyword (case-insensitive), `--focus-context` blocks around it, the heading
chain above it and the page title, and prints a summary of what was kept to
stderr. See [Markdown extraction](Markdown-extraction.md).

`--file` fetches every URL raw and prints one JSON status line per URL; it
only supports `--dump original` and cannot be combined with `--screenshot`.
For rendered output, run one `fetch` per URL.

The global `--proxy`, `--stealth`, `--obey-robots`, `--allow-private-network`
and `-v` flags also apply to `fetch`.

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

## `telemaco mcp`

Run telemaco as an MCP server.

```
    --http                   HTTP transport instead of stdio
    --host <HOST>            HTTP bind host (default 127.0.0.1)
    --port <PORT>            HTTP port (default 3000)
    --proxy <URL>            HTTP or SOCKS5 proxy
    --user-agent <UA>        Override the User-Agent
    --max-chars <N>          Cap on extracted page text (0 = unlimited)
    --agent-directives       Tell the agent on connect to reach the web through Telemaco
```

The global `--stealth`, `--allow-private-network` and `-v` flags also apply
to `mcp`.

`--max-chars` overrides the configured cap for `browser_markdown` and `browser_snapshot`; a `max_chars` argument on an individual tool call still wins over it. Every other extraction cap is set through the config file or the environment: see [Configuration file](Configuration-file.md).

`--host` only applies with `--http`. The default `127.0.0.1` keeps the server loopback-only; set `0.0.0.0` to bind all interfaces (for example a container sidecar) and pair it with `TELEMACO_MCP_ALLOWED_ORIGINS`.

Default transport is stdio. See [Use the MCP server](Use-the-MCP-server.md).

Render-enabled builds add `browser_screenshot` and `browser_pdf` to the MCP
tool list.

`--agent-directives` is off by default, so a server wired up by hand answers
`initialize` with plain capabilities. `telemaco install` adds it when you
accept it.

## `telemaco install`

Wire Telemaco into the coding agents on the machine: the MCP server, an
instructions block in the agent's memory file, and a prompt hook.

```
-t, --target <IDS>           Comma-separated agent ids, or auto | all | none
-l, --location <LOCATION>    global | local
-f, --folder <DIR>           Project directory, or home directory for a global install
-y, --yes                    Non-interactive, accept defaults
    --stealth                Put --stealth in the agent's MCP command
    --no-permissions         Do not auto-approve Telemaco tools
    --no-block-web           Leave the agent's own web search/fetch enabled
    --no-prompt-hook         Skip the prompt hook
    --dry-run                Report the plan without writing
    --print-config <AGENT>   Print the MCP snippet for one agent and exit
```

`--dir` is accepted as an alias of `--folder`. See the
[README](../README.md#agent-setup-telemaco-install) for what each part does.

## `telemaco uninstall`

Remove what `telemaco install` added.

```
-t, --target <IDS>           Comma-separated agent ids, or auto | all | none
-l, --location <LOCATION>    global | local
-f, --folder <DIR>           Project directory, or home directory of a global install
-y, --yes                    Non-interactive, remove from every detected agent
    --dry-run                Report what would be removed without writing
```

## `telemaco update`

Replace the binary with the latest release, keeping the build variant.

```
    --check                  Report only; exit 1 means an update is available
    --force                  Reinstall even when already on the latest version
```
