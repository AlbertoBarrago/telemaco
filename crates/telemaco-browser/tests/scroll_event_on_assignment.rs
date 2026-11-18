//! Assigning `scrollTop` / `scrollLeft` must fire a `scroll` event.
//!
//! `scrollTo`/`scrollBy` already dispatched one; direct assignment did not, so
//! the very common lazy-load idiom
//!
//! ```js
//! el.addEventListener('scroll', loadMore);
//! el.scrollTop = el.scrollHeight;      // never reached loadMore
//! ```
//!
//! silently did nothing and scroll-driven feeds stalled after their first batch.
//!
//! Checked against a real Chrome with this same probe: assigning `scrollTop`
//! fires exactly one `scroll` event when the element can actually scroll. The
//! fixture therefore has explicit viewport and content dimensions; Chromium
//! clamps a non-overflowing element to zero and dispatches no event.
//!
//! Ported from the telemaco-cdp suite to the Page API when the CDP server was
//! removed; the assertions are unchanged.

use std::io::{Read, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use telemaco_browser::{BrowserContext, Page};

const PAGE: &str = r#"<html><head><style>
    #box { width: 100px; height: 100px; overflow: scroll; }
    #content { width: 1000px; height: 2000px; }
</style></head><body><div id="box"><div id="content"><p>a</p><p>b</p></div></div></body></html>"#;

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
        "scroll-events".to_string(),
        None,
        false,
        None,
        None,
        true,
    ));
    let mut page = Page::new("scroll-events-page".to_string(), context);
    page.navigate(&url).await.unwrap();
    page
}

/// Evaluates `expression`, awaits the promise it returns by driving the page's
/// event loop, and parses the JSON string it resolves to. Awaiting matters
/// here: the scroll event is dispatched from a `setTimeout(..., 0)`, so the
/// probe has to yield before reading the counter.
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

async fn probe(page: &mut Page, body: &str) -> Value {
    let expression = format!(
        r#"(() => {{
            const el = document.getElementById('box');
            let fired = 0;
            el.addEventListener('scroll', () => {{ fired++; }});
            {body}
            return new Promise(r => setTimeout(() => r(JSON.stringify({{
                fired, top: el.scrollTop, left: el.scrollLeft,
            }})), 0));
        }})()"#
    );
    eval_awaited_json(page, &expression).await
}

#[tokio::test(flavor = "current_thread")]
async fn assigning_scroll_top_fires_one_scroll_event() {
    let mut page = open().await;
    let r = probe(&mut page, "el.scrollTop = 100;").await;
    assert_eq!(r["fired"], 1, "scrollTop assignment must fire exactly one scroll event");
    assert_eq!(r["top"], 100, "scrollTop must round-trip the assigned value");
}

#[tokio::test(flavor = "current_thread")]
async fn assigning_scroll_left_fires_one_scroll_event() {
    let mut page = open().await;
    let r = probe(&mut page, "el.scrollLeft = 40;").await;
    assert_eq!(r["fired"], 1, "scrollLeft assignment must fire exactly one scroll event");
    assert_eq!(r["left"], 40, "scrollLeft must round-trip the assigned value");
}

/// Re-assigning the same offset is not a scroll, so it must stay silent;
/// otherwise a loader that writes `scrollTop` on every frame re-enters forever.
#[tokio::test(flavor = "current_thread")]
async fn reassigning_the_same_offset_is_silent() {
    let mut page = open().await;
    let r = probe(&mut page, "el.scrollTop = 100; el.scrollTop = 100;").await;
    assert_eq!(r["fired"], 1, "only the offset change fires; the repeat must not");
}

/// `scrollTo` moves both axes, and a real browser reports one scroll per
/// movement rather than one per axis.
#[tokio::test(flavor = "current_thread")]
async fn scroll_to_coalesces_both_axes_into_one_event() {
    let mut page = open().await;
    let r = probe(&mut page, "el.scrollTo(30, 60);").await;
    assert_eq!(r["fired"], 1, "scrollTo must fire one event, not one per axis");
    assert_eq!(r["top"], 60);
    assert_eq!(r["left"], 30);
}

/// The lazy-load idiom the fix exists for: a listener that appends more rows
/// when the feed is scrolled. Before the fix this counted 0 and feeds froze.
#[tokio::test(flavor = "current_thread")]
async fn scroll_driven_lazy_loader_advances() {
    let mut page = open().await;
    let r = eval_awaited_json(
        &mut page,
        r#"(() => {
            const el = document.getElementById('box');
            let batches = 0;
            el.addEventListener('scroll', () => {
                if (batches >= 3) return;
                batches++;
                for (let i = 0; i < 5; i++) el.appendChild(document.createElement('p'));
            });
            el.scrollTop = 500;
            return new Promise(r => setTimeout(() => {
                el.scrollTop = 1000;
                setTimeout(() => r(JSON.stringify({
                    batches, rows: el.querySelectorAll('p').length,
                })), 0);
            }, 0));
        })()"#,
    )
    .await;
    assert_eq!(r["batches"], 2, "each forward scroll must reach the loader");
    assert_eq!(r["rows"], 12, "2 starting rows + 2 batches of 5");
}
