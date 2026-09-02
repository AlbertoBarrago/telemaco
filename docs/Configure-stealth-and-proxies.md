## Stealth mode

```bash
telemaco fetch https://example.com --stealth
telemaco serve --stealth
telemaco scrape url1 url2 --stealth
telemaco mcp --stealth
```

`--stealth` is a global flag, so it works before or after the subcommand and applies to `fetch`, `serve`, `scrape`, and `mcp`. In a `scrape` run each worker inherits it.

What `--stealth` changes:

- Uses the wreq HTTP client with browser-matching TLS fingerprints (ClientHello, ALPN, cipher order).
- Loads a tracker blocklist that drops requests to known analytics and fingerprinting endpoints.
- Bundles webpki roots instead of relying on the system store.

Requires a build that includes the stealth feature. Use a `-stealth` archive
with rendering or a `-no-render-stealth` archive without it. To build the
rendering variant yourself:

```bash
cargo build --release -p telemaco-cli --bins --features render,stealth
```

Omit rendering with `cargo build --release -p telemaco-cli --bins --no-default-features --features stealth`.

## What stealth handles

- Basic bot detection that checks TLS fingerprint or User-Agent.
- Sites that rely on third-party analytics being reachable.

## What stealth does not handle

- Cloudflare interactive challenges.
- Datadome and Akamai bot manager active challenges.
- CAPTCHAs.
- IP-based rate limiting (use proxies).

## Proxies

HTTP proxy:

```bash
telemaco fetch https://example.com --proxy http://proxy.example.com:8080
telemaco serve --proxy http://proxy.example.com:8080
```

With auth:

```bash
telemaco fetch https://example.com --proxy http://user:pass@proxy.example.com:8080
```

SOCKS5:

```bash
telemaco fetch https://example.com --proxy socks5://proxy.example.com:1080
```

## Custom User-Agent

```bash
telemaco fetch https://example.com --user-agent "Mozilla/5.0 (...) ..."
telemaco serve --user-agent "Mozilla/5.0 (...) ..."
```

Default UA matches a recent Chrome on the build platform.

## Browser profile, timezone, and geolocation

The engine presents one of a built-in pool of realistic browser profiles (a mix of Windows and macOS, recent Chrome versions). Each profile keeps `navigator.platform`, `navigator.userAgentData` (platform and platform version), and the UA string internally consistent, so the surfaces a site fingerprints agree with each other. There is no GPU renderer among them: `canvas.getContext('webgl')` returns `null`, so a page cannot read a renderer string at all.

A single stable profile is used by default. One IP cycling through different identities is itself a signal, so rotation is opt-in:

```bash
TELEMACO_PROFILE=2 telemaco serve          # pin a specific profile by index
TELEMACO_ROTATE_PROFILE=1 telemaco serve   # random profile per browser context
```

Timezone is driven by the process zone so `Date` (`getTimezoneOffset`, `toString`) and `Intl.DateTimeFormat` report the same region. Default is `Europe/Berlin`; set it to match the exit IP:

```bash
TELEMACO_TIMEZONE=America/New_York telemaco serve
```

`navigator.geolocation` reports configurable coordinates. Set them as `lat,lon` and keep them consistent with the timezone and proxy region:

```bash
TELEMACO_GEOLOCATION="40.7128,-74.0060" telemaco serve
```

Keep these aligned. A rotated or mismatched profile carries no matching TLS or timezone fingerprint, so when you pin a proxy region or TLS fingerprint, leave rotation off and set the timezone and geolocation to the same region. See [Environment variables](Environment-variables.md) for the full list.

## Combine

```bash
telemaco serve \
  --stealth \
  --proxy http://user:pass@proxy.example.com:8080 \
  --user-agent "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 ..."
```
