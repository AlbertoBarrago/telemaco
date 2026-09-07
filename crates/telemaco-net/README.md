# telemaco-net

HTTP client, cookie jar, stealth transport and SSRF guard for the
[Telemaco](https://github.com/AlbertoBarrago/telemaco) headless browser.

## Place in the workspace

The networking layer of the Telemaco workspace. It owns the default `reqwest`
client (`client.rs`), the optional stealth client built on `wreq`/BoringSSL
(`wreq_client.rs`), the cookie jar, response decoding, the request/response
callback registry used for interception, the `robots.txt` cache and the tracker
blocklist. It depends on no other Telemaco crate. `telemaco-js` and
`telemaco-browser` sit above it and route every network fetch through it.

## Features

| Feature | Effect |
|---------|--------|
| `stealth` | Pulls in `wreq`, `wreq-util` and `futures-util`, and exposes `StealthHttpClient`: a BoringSSL transport that presents a consistent Chrome TLS fingerprint and client hints. Needs `cmake`. Off by default, in which case the `reqwest`/rustls client is used. |

## Usage

```rust,no_run
use telemaco_net::TelemacoHttpClient;
use url::Url;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = TelemacoHttpClient::new();
    let response = client.fetch(&Url::parse("https://example.com")?).await?;
    println!("{} {}", response.status, response.text().len());
    Ok(())
}
```

`Response::text()` decodes the body using the `Content-Type` charset, then a
sniffed `<meta charset>` for HTML, then UTF-8.

## Invariants

- **SSRF guard on by default.** Loopback, RFC1918 and link-local destinations
  are refused, before and after redirects (`is_forbidden_ip`,
  `env_allows_private_network`). Opt in with `TELEMACO_ALLOW_PRIVATE_NETWORK=1`,
  or `--allow-private-network` on the CLI, and only for deliberate local
  testing.
- Any new code path that rewrites a request URL must re-run the same validation,
  or it becomes a way around the guard.

Part of [Telemaco](https://github.com/AlbertoBarrago/telemaco).
