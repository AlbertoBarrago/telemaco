Telemaco reads an optional TOML configuration file. Today it carries the MCP extraction limits; the file is the lowest of the explicit configuration layers, so anything you set in it can still be overridden per process or per call.

## Where it lives

Searched in order, first match wins:

1. the path given to `--config`
2. `./telemaco.toml`
3. `~/.config/telemaco/config.toml`

A path passed to `--config` **must** exist: naming a file that is not there is an error, because silently ignoring it would hide the mistake. The two discovered locations are optional, and their absence simply means built-in defaults.

A file that is found but is malformed, or that carries an unknown key or section, is a hard error at startup. A broken config that quietly does nothing is worse than a server that refuses to start, and an ignored typo is exactly the failure this rule prevents.

## Precedence

```text
tool call argument > CLI flag > environment variable > config file > default
```

So a `max_chars` argument on a single `browser_markdown` call beats `--max-chars`, which beats `TELEMACO_MCP_MAX_CHARS`, which beats the file, which beats the built-in default.

## MCP extraction limits

```toml
[mcp.limits]
max_chars = 20000
max_links = 200
```

Only the keys you set are changed; the rest keep their defaults.

| Key | Controls | Default |
|---|---|---|
| `max_chars` | characters per page from `browser_markdown`, and the text cap for `browser_snapshot` | 4000 |
| `max_links` | anchors from `browser_links` | 100 |
| `max_interactive` | elements from `browser_interactive_elements` | 100 |
| `max_search_results` | matches from `browser_search` | 10 |
| `search_context_chars` | characters around each `browser_search` match | 80 |
| `max_network_requests` | entries from `browser_network_requests` | 500 |
| `max_console_messages` | entries from `browser_console_messages` | 500 |
| `max_forms` | forms from `browser_detect_forms` | 100 |

`0` means unlimited on every one of them.

## Why there are limits at all

MCP output lands directly in an agent's context window. A single uncapped page dump can consume a whole window, which is why the defaults are conservative and raising them is a deliberate act rather than the built-in behavior.

When a result is capped, the tool says so. `browser_markdown` paginates at block boundaries and closes every page with a marker naming the page and the total, so the agent knows to ask for the next one. `browser_snapshot` appends the `...(truncated, N more chars)` marker, and list results a line naming how many entries were omitted out of the total. A capped result never looks like a complete one.

## A worked example

Reading long articles in full, while keeping the other caps modest:

```toml
[mcp.limits]
max_chars = 0        # one page holding the whole document, however long
max_links = 50       # but do not enumerate every anchor
```

For `browser_markdown`, `0` means the document is packed into a single page rather than split; the agent gets everything in one call and pays for it in context.

```bash
telemaco --config ./telemaco.toml mcp
```

The advertised tool schemas reflect the resolved values, so a client that inspects `tools/list` sees the caps this server will actually apply rather than the shipped defaults.

A fully commented reference file is at [`telemaco.example.toml`](../telemaco.example.toml).
