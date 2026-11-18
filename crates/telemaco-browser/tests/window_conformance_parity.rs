//! Conformance parity: a handful of standard Web APIs that real Chrome
//! exposes but bootstrap.js left undefined, so vendor JS that feature-detects
//! them dies with "X is not a function". These are spec surface gaps, not
//! anti-bot work.
//!
//! Ported from the telemaco-cdp suite to the Page API when the CDP server was
//! removed; the assertions are unchanged.

use std::io::{Read, Write};
use std::sync::Arc;

use serde_json::Value;
use telemaco_browser::{BrowserContext, Page};

const PAGE: &str = "<html><body><div id=a></div></body></html>";

fn serve() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else {
                continue;
            };
            std::thread::spawn(move || {
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{PAGE}",
                    PAGE.len()
                );
            });
        }
    });
    format!("http://{address}/")
}

async fn open() -> Page {
    std::env::set_var("TELEMACO_ALLOW_PRIVATE_NETWORK", "1");
    let url = serve();
    let context = Arc::new(BrowserContext::with_storage_and_network(
        "window-conformance".to_string(),
        None,
        false,
        None,
        None,
        true,
    ));
    let mut page = Page::new("window-conformance-page".to_string(), context);
    page.navigate(&url).await.unwrap();
    page
}

fn eval_json(page: &mut Page, expression: &str) -> Value {
    let raw = page.evaluate(expression);
    serde_json::from_str(raw.as_str().unwrap_or_else(|| panic!("expected a JSON string, got {raw}")))
        .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn element_toggle_attribute_is_callable_and_toggles() {
    let mut page = open().await;
    // Element.prototype.toggleAttribute is on every real Chrome. Frameworks
    // that call el.toggleAttribute() (Lit, Stencil, several ad SDKs) throw
    // without it.
    let val = eval_json(
        &mut page,
        r#"(function () {
            const el = document.getElementById('a');
            const t = typeof el.toggleAttribute;
            const first = el.toggleAttribute('hidden');
            const afterFirst = el.hasAttribute('hidden');
            const second = el.toggleAttribute('hidden');
            const afterSecond = el.hasAttribute('hidden');
            return JSON.stringify({ type: t, first, afterFirst, second, afterSecond });
        })()"#,
    );
    assert_eq!(val["type"], "function");
    assert_eq!(val["first"], true, "first toggle should add the attribute");
    assert_eq!(val["afterFirst"], true);
    assert_eq!(val["second"], false, "second toggle should remove it");
    assert_eq!(val["afterSecond"], false);
}

#[tokio::test(flavor = "current_thread")]
async fn document_adopt_node_moves_node() {
    let mut page = open().await;
    // document.adoptNode is standard DOM. Frameworks that move nodes between
    // documents (iframes, portals) call it.
    let val = eval_json(
        &mut page,
        r#"(function () {
            const t = typeof document.adoptNode;
            const span = document.createElement('span');
            span.id = 'moved';
            const adopted = document.adoptNode(span);
            return JSON.stringify({
                type: t,
                sameNode: adopted === span,
                ownerDoc: adopted.ownerDocument === document,
            });
        })()"#,
    );
    assert_eq!(val["type"], "function");
    assert_eq!(val["sameNode"], true);
    assert_eq!(val["ownerDoc"], true);
}
