//! Regression for issue #474: external scripts inserted after a timer must be
//! fetched and execute before an explicit post-navigation settle completes.
//! Also covers the fetch concurrency and execution order of dynamic classic
//! scripts, and the load/error events of dynamic `data:` scripts.
//!
//! Ported from the telemaco-cdp suite to the Page API when the CDP server was
//! removed; the assertions are unchanged. `Page::navigate` waits for `load`,
//! exactly like the suite's `Page.navigate` with `waitUntil: "load"`.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use telemaco_browser::{BrowserContext, Page};

const TIMER_PAGE: &str = r#"<html><body>
<div id="r">stage1</div>
<script>
setTimeout(function () {
  var direct = document.createElement("script");
  direct.src = "/direct.js";
  direct.onload = function () { window.__directLoaded = true; };
  document.body.appendChild(direct);

  var box = document.createElement("div");
  var nested = document.createElement("script");
  nested.src = "/nested.js";
  nested.onload = function () { window.__nestedLoaded = true; };
  box.appendChild(nested);
  document.body.appendChild(box);
}, 100);
</script>
</body></html>"#;

/// Answers every request on its own thread through `route`, which receives
/// the request line and returns the content type and body.
fn serve<F>(route: F) -> String
where
    F: Fn(&str) -> (&'static str, String) + Send + Sync + 'static,
{
    let route = Arc::new(route);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else {
                continue;
            };
            let route = route.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 2048];
                let read = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..read]).to_string();
                let (content_type, body) = route(&request);
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

fn new_page(name: &str) -> Page {
    std::env::set_var("TELEMACO_ALLOW_PRIVATE_NETWORK", "1");
    let context = Arc::new(BrowserContext::with_storage_and_network(
        name.to_string(),
        None,
        false,
        None,
        None,
        true,
    ));
    Page::new(format!("{name}-page"), context)
}

/// Evaluates `expression` and drives the event loop until the promise it
/// returns settles. Stands in for the suite's `Runtime.callFunctionOn` with
/// `awaitPromise: true`.
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

/// Serves a page that appends a slow and a fast external script, and records
/// the peak number of requests in flight at once.
fn serve_dynamic_order_fixture(explicitly_in_order: bool) -> (String, Arc<AtomicUsize>) {
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let server_peak = peak.clone();
    let url = serve(move |request| {
        let current = active.fetch_add(1, Ordering::SeqCst) + 1;
        server_peak.fetch_max(current, Ordering::SeqCst);
        let response = if request.starts_with("GET /slow.js") {
            std::thread::sleep(Duration::from_millis(700));
            (
                "application/javascript",
                "window.__dynamicOrder.push('slow');".to_string(),
            )
        } else if request.starts_with("GET /fast.js") {
            std::thread::sleep(Duration::from_millis(500));
            (
                "application/javascript",
                "window.__dynamicOrder.push('fast');".to_string(),
            )
        } else {
            let ordered = if explicitly_in_order {
                "slow.async=false;fast.async=false;"
            } else {
                ""
            };
            (
                "text/html",
                format!(
                    r#"<script>
window.__dynamicOrder=[];
var slow=document.createElement('script');
var fast=document.createElement('script');
{ordered}
slow.src='/slow.js';
fast.src='/fast.js';
document.head.appendChild(slow);
document.head.appendChild(fast);
</script>"#
                ),
            )
        };
        active.fetch_sub(1, Ordering::SeqCst);
        response
    });
    (url, peak)
}

async fn navigate_dynamic_order_fixture(explicitly_in_order: bool) -> (Vec<String>, usize, u128) {
    let (url, peak) = serve_dynamic_order_fixture(explicitly_in_order);
    let mut page = new_page("script-order");

    let started = Instant::now();
    page.navigate(&url).await.unwrap();
    let elapsed_ms = started.elapsed().as_millis();
    let order = page
        .evaluate("window.__dynamicOrder")
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_string())
        .collect();
    (order, peak.load(Ordering::SeqCst), elapsed_ms)
}

#[tokio::test(flavor = "current_thread")]
async fn dynamic_classic_fetch_concurrency_matches_force_async_state() {
    let (async_order, async_peak, async_elapsed_ms) =
        navigate_dynamic_order_fixture(false).await;
    assert_eq!(async_order, ["fast", "slow"]);
    assert_eq!(
        async_peak, 2,
        "default dynamic classics must fetch concurrently"
    );

    let (ordered_order, ordered_peak, ordered_elapsed_ms) =
        navigate_dynamic_order_fixture(true).await;
    assert_eq!(ordered_order, ["slow", "fast"]);
    assert_eq!(
        ordered_peak, 2,
        "explicit async=false scripts must overlap fetches while retaining their execution-order queue"
    );
    assert!(
        ordered_elapsed_ms < async_elapsed_ms + 300,
        "execution ordering must not serialize network fetches: async={async_elapsed_ms}ms ordered={ordered_elapsed_ms}ms"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn dynamic_external_scripts_execute_and_fire_load() {
    let url = serve(|request| {
        if request.starts_with("GET /direct.js") {
            std::thread::sleep(Duration::from_millis(600));
            ("application/javascript", "window.__directExecuted = true;".to_string())
        } else if request.starts_with("GET /nested.js") {
            std::thread::sleep(Duration::from_millis(50));
            ("application/javascript", "window.__nestedExecuted = true;".to_string())
        } else {
            ("text/html", TIMER_PAGE.to_string())
        }
    });
    let mut page = new_page("dynamic-scripts");
    page.navigate(&url).await.unwrap();

    let state = "JSON.stringify({directExecuted: !!window.__directExecuted, directLoaded: !!window.__directLoaded, nestedExecuted: !!window.__nestedExecuted, nestedLoaded: !!window.__nestedLoaded})";
    assert_eq!(
        page.evaluate(state),
        json!(r#"{"directExecuted":false,"directLoaded":false,"nestedExecuted":false,"nestedLoaded":false}"#),
        "waitUntil=load must not invent a post-load timer settle"
    );

    // The automation caller opts into post-load work. This must drive the
    // timer, both concurrently fetched scripts, their bodies, and their load
    // handlers without relying on navigation to exceed browser load semantics.
    page.settle(1_500).await;

    assert_eq!(
        page.evaluate(state),
        json!(r#"{"directExecuted":true,"directLoaded":true,"nestedExecuted":true,"nestedLoaded":true}"#),
        "dynamic scripts must execute and fire load before explicit settle completes"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn dynamic_data_scripts_execute_before_chained_load_handlers() {
    let mut page = new_page("data-scripts");
    page.navigate("data:text/html,<html><head></head><body></body></html>")
        .await
        .unwrap();

    page.evaluate(
        r#"(function () {
            var state = {
                aExec: false,
                aLoad: false,
                bExec: false,
                bLoad: false,
                cExec: false,
                cLoad: false
            };
            window.__dataScriptState = state;

            var a = document.createElement('script');
            a.src = 'data:,window.__dataScriptState.aExec=true';
            a.onload = function () {
                state.aLoad = true;
                var b = document.createElement('script');
                b.src = 'data:text/html,' + encodeURIComponent('window.__dataScriptState.bExec=true');
                b.onload = function () {
                    state.bLoad = true;
                    var c = document.createElement('script');
                    c.src = 'data:text/javascript;base64,' +
                        btoa('window.__dataScriptState.cExec=true').replace(/=+$/, '') +
                        '#ignored-fragment';
                    c.onload = function () { state.cLoad = true; };
                    document.head.appendChild(c);
                };
                document.head.appendChild(b);
            };
            document.head.appendChild(a);
            return 'kicked';
        })()"#,
    );

    page.settle(500).await;

    assert_eq!(
        page.evaluate("JSON.stringify(window.__dataScriptState)"),
        json!(r#"{"aExec":true,"aLoad":true,"bExec":true,"bLoad":true,"cExec":true,"cLoad":true}"#),
        "data URL script bodies must execute before each chained load handler"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_dynamic_data_script_fires_error_not_load() {
    let mut page = new_page("invalid-data-script");
    page.navigate("data:text/html,<html><head></head><body></body></html>")
        .await
        .unwrap();

    eval_awaited(
        &mut page,
        r#"(function () {
            window.__invalidDataScript = { error: false, load: false };
            var script = document.createElement('script');
            script.src = 'data:text/javascript;base64,!';
            script.onerror = function () { window.__invalidDataScript.error = true; };
            script.onload = function () { window.__invalidDataScript.load = true; };
            document.head.appendChild(script);
        })()"#,
    )
    .await;

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        page.evaluate("JSON.stringify(window.__invalidDataScript)"),
        json!(r#"{"error":true,"load":false}"#)
    );
}
