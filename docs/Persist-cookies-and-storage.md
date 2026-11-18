`--storage-dir` persists cookies and localStorage to disk so they survive across runs.

## CLI

```bash
telemaco fetch https://example.com --storage-dir ./telemaco-data
telemaco fetch https://example.com --storage-dir ./telemaco-data
```

The second invocation starts with the cookies and localStorage left by the first.

## Layout

Inside `./telemaco-data`:

- `cookies.json`: cookie jar in a stable format with `same_site`, `expires`, `http_only`, `secure`.
- `localStorage/<origin>.json`: one file per origin.

The format is stable. Inspect with `jq`:

```bash
jq '.[] | select(.domain == "example.com")' ./telemaco-data/cookies.json
```

## When state is written

- On clean process exit (Ctrl-C, SIGTERM).
- After every navigation completes.

## Login once, scrape many

```bash
telemaco fetch https://example.com/login --storage-dir ./session-1 --eval "..."
telemaco fetch https://example.com/account --storage-dir ./session-1
```

Log in once (with `--eval` filling the form, or by importing cookies), then later runs against the same `--storage-dir` start logged in. Use one directory per identity for isolated profiles.

## MCP sessions

The MCP server keeps state in memory. To reuse a login across runs, export it with `browser_storage_state` and restore it with `browser_set_storage_state`. See [Use the MCP server](Use-the-MCP-server.md).

## Clear state

```bash
rm -rf ./telemaco-data
```
