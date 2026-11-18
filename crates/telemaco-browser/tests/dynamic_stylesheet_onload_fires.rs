//! Regression for issue #409: a `<link rel="stylesheet" href>` inserted via JS
//! (createElement + appendChild) must fetch and fire `load`, so frameworks that
//! await the link's onload (Promise.all of lazy CSS + JS, antd/bootstrap
//! loaders) resolve instead of hanging forever. Without the fix the link fires
//! neither load nor error, so the page stays on stage1.
//!
//! Ported from the telemaco-cdp suite to the Page API when the CDP server was
//! removed; the assertions are unchanged.

use std::io::{Read, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use telemaco_browser::{BrowserContext, Page};

const PAGE: &str = r#"<html><head></head><body>
<div id="r">stage1</div>
<script>
window.__loaded = new Promise(function (resolve, reject) {
  var l = document.createElement("link");
  l.rel = "stylesheet";
  l.href = "/style.css";
  l.onload = function () { document.getElementById("r").textContent = "stage2"; resolve("ok"); };
  l.onerror = function () { reject("err"); };
  document.head.appendChild(l);
});
</script>
</body></html>"#;

/// Serves the page and the stylesheet it appends, so the CSS fetch succeeds.
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
                let read = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..read]);
                let (content_type, body) = if request.starts_with("GET /style.css") {
                    ("text/css", "body { color: red; }")
                } else {
                    ("text/html", PAGE)
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            });
        }
    });
    format!("http://{address}/")
}

/// Evaluates `expression`, awaits the promise it returns by driving the page's
/// event loop, and returns the resolved value. This replaces the CDP suite's
/// `Runtime.evaluate` with `awaitPromise: true`.
async fn eval_awaited(page: &mut Page, expression: &str) -> Value {
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
    page.evaluate("window.__portedValue")
}

#[tokio::test(flavor = "current_thread")]
async fn dynamic_stylesheet_fires_load() {
    std::env::set_var("TELEMACO_ALLOW_PRIVATE_NETWORK", "1");
    let url = serve();
    let context = Arc::new(BrowserContext::with_storage_and_network(
        "dynamic-stylesheet".to_string(),
        None,
        false,
        None,
        None,
        true,
    ));
    let mut page = Page::new("dynamic-stylesheet-page".to_string(), context);
    page.navigate(&url).await.unwrap();

    // Race the link's onload promise against a 5s timeout. Without the fix the
    // link never settles, so this rejects with "timeout" and the assertion fails.
    let value = eval_awaited(
        &mut page,
        "(async () => { try { return await Promise.race([window.__loaded, new Promise((_, r) => setTimeout(() => r('timeout'), 5000))]); } catch (e) { return 'rejected:' + e; } })()",
    )
    .await;
    assert_eq!(value, json!("ok"), "dynamic <link rel=stylesheet> must fire load (got {value:?})");

    assert_eq!(
        page.evaluate("document.getElementById('r').textContent"),
        json!("stage2"),
        "the page must advance to stage2 once the stylesheet link loads"
    );
}
