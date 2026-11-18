//! Regression tests for iframe event dispatch. `_IframeDocument`'s
//! addEventListener/removeEventListener/dispatchEvent were no-ops, so listeners
//! registered on an iframe document never ran. Separately, iframe load invoked
//! the `onload` property directly instead of dispatching through the element,
//! which skipped any addEventListener('load', ...) listener.
//!
//! Ported from the telemaco-cdp suite to the Page API when the CDP server was
//! removed; the assertions are unchanged.

use std::io::{Read, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use telemaco_browser::{BrowserContext, Page};

const PAGE: &str = "<html><body><div id=a></div></body></html>";

/// Serves the same document for every request: the load test points the
/// iframe at the page's own URL, so the server must answer more than once.
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
        "iframe-events".to_string(),
        None,
        false,
        None,
        None,
        true,
    ));
    let mut page = Page::new("iframe-events-page".to_string(), context);
    page.navigate(&url).await.unwrap();
    page
}

/// Evaluates `expression`, awaits the promise it returns by driving the page's
/// event loop, and parses the JSON string it resolves to. This replaces the
/// CDP suite's `Runtime.evaluate` with `awaitPromise: true`; a rejection comes
/// back as a `"rejected: ..."` string, which fails the JSON parse loudly.
async fn eval_awaited_json(page: &mut Page, expression: &str) -> Value {
    page.evaluate(&format!(
        "(() => {{ window.__portedDone = false; Promise.resolve(({expression})).then(\
         v => {{ window.__portedValue = v; window.__portedDone = true; }},\
         e => {{ window.__portedValue = 'rejected: ' + e; window.__portedDone = true; }}); }})()"
    ));
    let deadline = Instant::now() + Duration::from_secs(10);
    while page.evaluate("window.__portedDone") != json!(true) {
        assert!(Instant::now() < deadline, "the evaluated promise never settled");
        page.settle(50).await;
    }
    let raw = page.evaluate("window.__portedValue");
    serde_json::from_str(raw.as_str().unwrap_or_else(|| panic!("expected a JSON string, got {raw}")))
        .unwrap_or_else(|error| panic!("expected JSON, got {raw}: {error}"))
}

#[tokio::test(flavor = "current_thread")]
async fn iframe_document_dispatches_registered_listeners() {
    let mut page = open().await;
    // Accessing contentDocument on a src-less iframe lazily creates an
    // about:blank _IframeDocument synchronously, so this exercises the event
    // target without waiting on an async load.
    let val = eval_awaited_json(
        &mut page,
        r#"(function () {
            const iframe = document.createElement('iframe');
            document.body.appendChild(iframe);
            const doc = iframe.contentDocument;
            const out = { hasDoc: !!doc };
            let calls = 0;
            const listener = () => { calls++; };
            // A registered listener runs on dispatch.
            doc.addEventListener('probe', listener);
            doc.dispatchEvent(new Event('probe'));
            out.afterRegister = calls;
            // Registering the same listener again is a no-op (deduped).
            doc.addEventListener('probe', listener);
            doc.addEventListener('probe', listener);
            doc.dispatchEvent(new Event('probe'));
            out.afterDuplicate = calls;
            // After removal the listener no longer runs.
            doc.removeEventListener('probe', listener);
            doc.dispatchEvent(new Event('probe'));
            out.afterRemove = calls;
            // dispatchEvent reports cancellation via its return value.
            doc.addEventListener('cancelme', (e) => e.preventDefault());
            out.cancelReturn = doc.dispatchEvent(new Event('cancelme', { cancelable: true }));
            out.plainReturn = doc.dispatchEvent(new Event('nolisteners'));
            return JSON.stringify(out);
        })()"#,
    )
    .await;
    assert_eq!(val["hasDoc"], true);
    assert_eq!(val["afterRegister"].as_u64(), Some(1), "listener runs once on dispatch");
    assert_eq!(val["afterDuplicate"].as_u64(), Some(2), "duplicate registration deduped");
    assert_eq!(val["afterRemove"].as_u64(), Some(2), "removed listener does not run");
    assert_eq!(val["cancelReturn"], false, "preventDefault -> dispatchEvent returns false");
    assert_eq!(val["plainReturn"], true, "no cancellation -> dispatchEvent returns true");
}

#[tokio::test(flavor = "current_thread")]
async fn iframe_load_reaches_onload_and_addeventlistener() {
    let mut page = open().await;
    // Both the onload property and an addEventListener('load', ...) listener
    // must run exactly once. Without the fix, setting onload made the
    // direct-call path skip the addEventListener listener entirely.
    let val = eval_awaited_json(
        &mut page,
        r#"(function () {
            return new Promise((resolve) => {
                const iframe = document.createElement('iframe');
                const events = [];
                // Resolve from the onload property one microtask later, so the
                // synchronous addEventListener('load') handler has already run
                // on a fixed build. Without the fix the listener never fires,
                // so this reports ["property"] instead of hanging.
                iframe.onload = () => {
                    events.push('property');
                    Promise.resolve().then(() => resolve(JSON.stringify({ events })));
                };
                iframe.addEventListener('load', () => events.push('listener'));
                document.body.appendChild(iframe);
                iframe.src = location.href;
            });
        })()"#,
    )
    .await;
    assert_eq!(
        val["events"],
        json!(["property", "listener"]),
        "both onload property and addEventListener('load') run exactly once, in order"
    );
}
