//! `Page::base_url` must follow `<base href>`, not the document URL.
//!
//! Anything that turns a page's relative href or src into an absolute URL has
//! to resolve against the document base URL. Using the document URL instead
//! produces a target that looks plausible and points at the wrong place, which
//! is why this is pinned: the CLI link and asset dumps got it wrong for exactly
//! that reason, and nothing failed loudly when they did.

use std::io::{Read, Write};
use std::sync::Arc;

use telemaco_browser::{BrowserContext, Page};

/// Serve one fixed HTML document on loopback, once per connection.
fn spawn_page_server(body: &'static str) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();

    std::thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { continue };
            std::thread::spawn(move || {
                // Read the request head so the client is never left writing
                // into a socket nobody drains.
                let mut request = Vec::new();
                let mut chunk = [0u8; 1024];
                loop {
                    let Ok(read) = stream.read(&mut chunk) else { return };
                    if read == 0 {
                        return;
                    }
                    request.extend_from_slice(&chunk[..read]);
                    if request.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            });
        }
    });

    format!("http://{}:{}/docs/page.html", address.ip(), address.port())
}

async fn base_url_of(body: &'static str) -> String {
    std::env::set_var("TELEMACO_ALLOW_PRIVATE_NETWORK", "1");
    let context = Arc::new(BrowserContext::new("base-url-test".to_string()));
    let mut page = Page::new("base-url-test-page".to_string(), context);
    let url = spawn_page_server(body);
    page.navigate(&url).await.expect("navigation");
    page.base_url().expect("a base url").to_string()
}

#[tokio::test(flavor = "current_thread")]
async fn base_url_follows_an_absolute_base_href() {
    let base = base_url_of(
        "<!DOCTYPE html><html><head><base href=\"/sub/\"></head><body>x</body></html>",
    )
    .await;
    assert!(
        base.ends_with("/sub/"),
        "a <base href> must win over the document URL, got {base}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn base_url_falls_back_to_the_document_url() {
    let base = base_url_of("<!DOCTYPE html><html><head></head><body>x</body></html>").await;
    assert!(
        base.ends_with("/docs/page.html"),
        "without a <base href> the document URL is the base, got {base}"
    );
}
