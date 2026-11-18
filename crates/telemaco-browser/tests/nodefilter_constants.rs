//! `NodeFilter` must expose the standard filter constants (issue #439).
//! bootstrap.js defined NodeFilter twice — a partial live one (SHOW_ELEMENT /
//! SHOW_TEXT / SHOW_ALL) and a complete one behind a dead `typeof === undefined`
//! guard — so `NodeFilter.FILTER_ACCEPT` was undefined at runtime and the
//! canonical `acceptNode() { return NodeFilter.FILTER_ACCEPT; }` idiom made the
//! walker reject every node.
//!
//! Ported from the telemaco-cdp suite to the Page API when the CDP server was
//! removed; the assertions are unchanged.

use std::io::{Read, Write};
use std::sync::Arc;

use serde_json::Value;
use telemaco_browser::{BrowserContext, Page};

const PAGE: &str = "<html><body><script>window.__boot = true;</script></body></html>";

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
        "nodefilter".to_string(),
        None,
        false,
        None,
        None,
        true,
    ));
    let mut page = Page::new("nodefilter-page".to_string(), context);
    page.navigate(&url).await.unwrap();
    page
}

fn eval_json(page: &mut Page, expression: &str) -> Value {
    let raw = page.evaluate(expression);
    serde_json::from_str(raw.as_str().unwrap_or_else(|| panic!("expected a JSON string, got {raw}")))
        .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn node_filter_exposes_the_standard_constants() {
    let mut page = open().await;
    let val = eval_json(
        &mut page,
        r#"JSON.stringify({
            accept: NodeFilter.FILTER_ACCEPT,
            reject: NodeFilter.FILTER_REJECT,
            skip: NodeFilter.FILTER_SKIP,
            showAll: NodeFilter.SHOW_ALL,
            showElement: NodeFilter.SHOW_ELEMENT,
            showText: NodeFilter.SHOW_TEXT,
            showComment: NodeFilter.SHOW_COMMENT,
        })"#,
    );
    assert_eq!(val["accept"], 1, "FILTER_ACCEPT must be 1");
    assert_eq!(val["reject"], 2, "FILTER_REJECT must be 2");
    assert_eq!(val["skip"], 3, "FILTER_SKIP must be 3");
    assert_eq!(val["showAll"], 0xFFFFFFFFu32, "SHOW_ALL must be 0xFFFFFFFF");
    assert_eq!(val["showElement"], 1, "SHOW_ELEMENT must be 1");
    assert_eq!(val["showText"], 4, "SHOW_TEXT must be 4");
    assert_eq!(val["showComment"], 128, "SHOW_COMMENT must be 128");
}

#[tokio::test(flavor = "current_thread")]
async fn tree_walker_filter_using_filter_accept_constant_works() {
    let mut page = open().await;
    // The canonical MDN idiom: acceptNode returns NodeFilter.FILTER_ACCEPT.
    // Only the first accepted child is checked, so this does not depend on the
    // separate nextNode leaf-advance fix (#432).
    let val = eval_json(
        &mut page,
        r#"(() => {
            document.body.innerHTML = '<div id="r"><p id="target">keep</p></div>';
            const r = document.getElementById('r');
            const w = document.createTreeWalker(r, NodeFilter.SHOW_ELEMENT, {
                acceptNode() { return NodeFilter.FILTER_ACCEPT; }
            });
            const first = w.nextNode();
            return JSON.stringify({ id: first ? first.id : null });
        })()"#,
    );
    assert_eq!(
        val["id"], "target",
        "a filter returning NodeFilter.FILTER_ACCEPT must accept the node, not reject it"
    );
}
