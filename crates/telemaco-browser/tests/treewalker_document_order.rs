//! `TreeWalker.nextNode` must walk the whole subtree in document order, keep
//! searching after filtered leaf nodes (#432), and handle a deep chain of
//! accepted children without recursing per level.
//!
//! Ported from the telemaco-cdp suite to the Page API when the CDP server was
//! removed; the assertions are unchanged.

use std::io::{Read, Write};
use std::sync::Arc;

use telemaco_browser::{BrowserContext, Page};

const PAGE: &str = "<html><body></body></html>";

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
        "treewalker".to_string(),
        None,
        false,
        None,
        None,
        true,
    ));
    let mut page = Page::new("treewalker-page".to_string(), context);
    page.navigate(&url).await.unwrap();
    page
}

fn eval_strings(page: &mut Page, expression: &str) -> Vec<String> {
    let raw = page.evaluate(expression);
    serde_json::from_str(raw.as_str().unwrap_or_else(|| panic!("expected a JSON string, got {raw}")))
        .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn next_node_walks_the_whole_subtree_in_document_order() {
    let mut page = open().await;
    let seen = eval_strings(
        &mut page,
        r#"(() => {
            document.body.innerHTML =
              '<div id="r"><span></span><p>hi</p><section><a></a><b></b></section></div>';
            const walker = document.createTreeWalker(
              document.getElementById('r'), NodeFilter.SHOW_ELEMENT);
            const seen = [];
            let node;
            while ((node = walker.nextNode())) seen.push(node.tagName);
            return JSON.stringify(seen);
        })()"#,
    );
    assert_eq!(seen, ["SPAN", "P", "SECTION", "A", "B"]);
}

#[tokio::test(flavor = "current_thread")]
async fn next_node_keeps_searching_after_filtered_leaf_nodes() {
    let mut page = open().await;
    let seen = eval_strings(
        &mut page,
        r#"(() => {
            document.body.innerHTML =
              '<div id="r"><p>one</p><p>two</p><p>three</p></div>';
            const walker = document.createTreeWalker(
              document.getElementById('r'), NodeFilter.SHOW_TEXT, {
                acceptNode(node) {
                  return node.data === 'two'
                    ? NodeFilter.FILTER_REJECT
                    : NodeFilter.FILTER_ACCEPT;
                }
              });
            const seen = [];
            let node;
            while ((node = walker.nextNode())) seen.push(node.data);
            return JSON.stringify(seen);
        })()"#,
    );
    assert_eq!(seen, ["one", "three"]);
}

#[tokio::test(flavor = "current_thread")]
async fn next_node_handles_a_deep_accepted_child_fast_path() {
    let mut page = open().await;
    let value = page.evaluate(
        r#"(() => {
            const root = document.createElement('div');
            let parent = root;
            for (let i = 0; i < 5000; i++) {
              const child = document.createElement('span');
              parent.appendChild(child);
              parent = child;
            }
            document.body.appendChild(root);
            const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT);
            let count = 0;
            while (walker.nextNode()) count++;
            return count;
        })()"#,
    );
    assert_eq!(value.as_f64(), Some(5000.0));
}
