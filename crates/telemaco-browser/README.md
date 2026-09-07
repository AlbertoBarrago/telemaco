# telemaco-browser

Page navigation and JavaScript evaluation for the
[Telemaco](https://github.com/AlbertoBarrago/telemaco) headless browser.

## Place in the workspace

The page layer. It owns `BrowserContext` (cookies, storage, proxy and network
options shared by pages) and `Page` (navigation, lifecycle and wait conditions,
JS evaluation, request interception, and with `render`, screenshots and PDF). It
sits above `telemaco-dom`, `telemaco-net` and `telemaco-js`, and is what
`telemaco-cdp`, `telemaco-mcp`, `telemaco-cli` and the embeddable `telemaco`
crate call. Cross-crate calls go through the layer above, never sideways.

Most users want the friendlier
[`telemaco`](https://github.com/AlbertoBarrago/telemaco/tree/main/crates/telemaco)
crate, which wraps this one in a `Browser` builder.

## Features

| Feature | Effect |
|---------|--------|
| `stealth` | Enables `telemaco-net/stealth` and `telemaco-js/stealth`: the wreq/BoringSSL transport and matching browser identity for both navigation and scripted requests. |
| `render` | Enables `telemaco-js/render` and pulls in `image`: layout geometry, `Page::screenshot*` and the `pdf` module. |

## Usage

```rust,no_run
use std::sync::Arc;
use telemaco_browser::{BrowserContext, Page};

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let context = Arc::new(BrowserContext::new("default".to_string()));
let mut page = Page::new("page-1".to_string(), context);

page.navigate("https://example.com").await?;
page.settle(2000).await;

let title = page.evaluate("document.title");
println!("{title}");

let links = page
    .with_dom(|dom| dom.query_selector_all("a").unwrap_or_default().len())
    .unwrap_or(0);
println!("{links} links");
# Ok(())
# }
```

`Page` is `!Send`, so run it on a tokio `LocalSet`.

## Invariant

One V8 isolate exists per process, so concurrent page work must be serialized by
the caller. `telemaco-cdp` does this with a per-connection lock; the `telemaco`
crate does it for you.

Part of [Telemaco](https://github.com/AlbertoBarrago/telemaco).
