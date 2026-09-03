//! Click-to-focus and character typing over the CDP Input domain.
//!
//! The GUI (and any CDP client that clicks and types) must observe the
//! default actions a real browser performs: mousedown on a control focuses
//! it, so `document.activeElement` moves and later `Input.dispatchKeyEvent`
//! char events insert text into it. Fake terminals that are plain divs with
//! document-level keydown handlers must also see keydown/keypress/keyup for
//! printable characters, and a prevented keydown must suppress insertion the
//! way Chrome does.

use telemaco_cdp::dispatch::{dispatch, CdpContext};
use telemaco_cdp::types::CdpRequest;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn serve_fixture() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 2048];
        let _ = socket.read(&mut buf).await.unwrap();
        let body = r#"<!doctype html><html><head><style>
            html, body { margin: 0; font: 16px monospace }
            #terminal-input { display: block; width: 300px; height: 40px }
            #pad { width: 100px; height: 40px }
            #lbl { display: block; width: 200px; height: 40px }
            #field2 { display: block; width: 200px; height: 30px }
        </style></head><body>
          <form id="shell-form"><input id="terminal-input" name="command"></form>
          <div id="pad"></div>
          <label id="lbl" for="field2">label</label><input id="field2">
          <script>
            window.log = [];
            var inp = document.getElementById('terminal-input');
            inp.addEventListener('focus', function () { window.log.push('focus:terminal-input'); });
            inp.addEventListener('blur', function () { window.log.push('blur:terminal-input'); });
            document.addEventListener('focusin', function (e) { window.log.push('focusin:' + (e.target.id || e.target.tagName)); });
            document.addEventListener('keydown', function (e) { window.log.push('keydown:' + e.key + ':' + (e.isTrusted ? 't' : 'f')); });
            document.addEventListener('keypress', function (e) { window.log.push('keypress:' + e.key); });
            document.addEventListener('keyup', function (e) { window.log.push('keyup:' + e.key); });
            document.getElementById('shell-form').addEventListener('submit', function (e) {
              e.preventDefault();
              window.log.push('submit:' + inp.value);
            });
          </script>
        </body></html>"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = socket.write_all(response.as_bytes()).await;
    });
    format!("http://{addr}/")
}

async fn cdp(ctx: &mut CdpContext, id: u64, method: &str, params: Value, sid: &str) -> Value {
    let response = dispatch(
        &CdpRequest {
            id,
            method: method.to_string(),
            params,
            session_id: Some(sid.to_string()),
        },
        ctx,
    )
    .await;
    assert!(response.error.is_none(), "CDP {method} failed: {:?}", response.error);
    response.result.unwrap_or_else(|| json!({}))
}

async fn evaluate(ctx: &mut CdpContext, id: u64, expression: &str, sid: &str) -> Value {
    cdp(
        ctx,
        id,
        "Runtime.evaluate",
        json!({"expression": expression, "returnByValue": true, "awaitPromise": true}),
        sid,
    )
    .await
}

async fn setup() -> (CdpContext, String) {
    std::env::set_var("TELEMACO_ALLOW_PRIVATE_NETWORK", "1");
    let url = serve_fixture().await;
    let mut ctx = CdpContext::new();
    let page_id = ctx.create_page();
    let sid = "click-focus-typing-session";
    ctx.sessions.insert(sid.to_string(), page_id);
    cdp(&mut ctx, 1, "Page.navigate", json!({"url": url, "waitUntil": "load"}), sid).await;
    (ctx, sid.to_string())
}

/// Click the centre of `selector` the way a real pointer would.
async fn click_element(ctx: &mut CdpContext, id: u64, sid: &str, selector: &str) {
    let rect = evaluate(
        ctx,
        id,
        &format!(
            "JSON.stringify(document.querySelector('{selector}').getBoundingClientRect().toJSON())"
        ),
        sid,
    )
    .await;
    let rect: Value = serde_json::from_str(rect["result"]["value"].as_str().unwrap()).unwrap();
    let x = rect["x"].as_f64().unwrap() + rect["width"].as_f64().unwrap() / 2.0;
    let y = rect["y"].as_f64().unwrap() + rect["height"].as_f64().unwrap() / 2.0;
    for kind in ["mousePressed", "mouseReleased"] {
        cdp(
            ctx,
            id + 1,
            "Input.dispatchMouseEvent",
            json!({"type": kind, "x": x, "y": y, "button": "left", "clickCount": 1}),
            sid,
        )
        .await;
    }
}

async fn send_char(ctx: &mut CdpContext, id: u64, sid: &str, text: &str) {
    cdp(
        ctx,
        id,
        "Input.dispatchKeyEvent",
        json!({"type": "char", "text": text}),
        sid,
    )
    .await;
}

async fn log(ctx: &mut CdpContext, id: u64, sid: &str) -> Vec<String> {
    let v = evaluate(ctx, id, "JSON.stringify(window.log)", sid).await;
    serde_json::from_str(v["result"]["value"].as_str().unwrap()).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn click_focuses_input_chars_type_and_enter_submits() {
    let (mut ctx, sid) = setup().await;
    click_element(&mut ctx, 10, &sid, "#terminal-input").await;

    let active = evaluate(&mut ctx, 20, "document.activeElement && document.activeElement.id", &sid).await;
    assert_eq!(
        active["result"]["value"].as_str(),
        Some("terminal-input"),
        "mousedown on an input must focus it: {active}"
    );

    send_char(&mut ctx, 21, &sid, "h").await;
    send_char(&mut ctx, 22, &sid, "i").await;

    let value = evaluate(&mut ctx, 23, "document.getElementById('terminal-input').value", &sid).await;
    assert_eq!(value["result"]["value"].as_str(), Some("hi"), "chars must insert into the focused input: {value}");

    let events = log(&mut ctx, 24, &sid).await;
    assert!(
        events.contains(&"focus:terminal-input".to_string())
            && events.contains(&"focusin:terminal-input".to_string()),
        "clicking an input fires focus and focusin: {events:?}"
    );
    assert!(
        events.iter().any(|e| e == "keydown:h:t"),
        "char events surface as trusted keydown: {events:?}"
    );
    assert!(
        events.iter().any(|e| e == "keypress:h") && events.iter().any(|e| e == "keyup:h"),
        "printable chars fire keypress and keyup: {events:?}"
    );

    cdp(
        &mut ctx,
        25,
        "Input.dispatchKeyEvent",
        json!({"type": "keyDown", "key": "Enter", "code": "Enter"}),
        &sid,
    )
    .await;

    let events = log(&mut ctx, 26, &sid).await;
    assert!(
        events.iter().any(|e| e == "submit:hi"),
        "Enter submits the form holding the focused input: {events:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn chars_reach_document_keydown_without_a_focused_input() {
    // Fake shells are often plain divs with document-level keydown handlers:
    // typing must still produce the keyboard events even though there is no
    // input element to insert into, and clicking a non-focusable div must not
    // move focus anywhere.
    let (mut ctx, sid) = setup().await;
    click_element(&mut ctx, 10, &sid, "#pad").await;

    let active = evaluate(&mut ctx, 20, "document.activeElement && document.activeElement.tagName", &sid).await;
    assert_eq!(active["result"]["value"].as_str(), Some("BODY"), "clicking a plain div leaves focus alone: {active}");

    send_char(&mut ctx, 21, &sid, "o").await;
    send_char(&mut ctx, 22, &sid, "k").await;

    let events = log(&mut ctx, 23, &sid).await;
    for expected in ["keydown:o:t", "keypress:o", "keyup:o", "keydown:k:t", "keypress:k", "keyup:k"] {
        assert!(
            events.iter().any(|e| e == expected),
            "shell div must see {expected}: {events:?}"
        );
    }

    let value = evaluate(&mut ctx, 24, "document.getElementById('terminal-input').value", &sid).await;
    assert_eq!(value["result"]["value"].as_str(), Some(""), "nothing may be inserted into an unfocused field: {value}");
}

#[tokio::test(flavor = "current_thread")]
async fn prevented_keydown_suppresses_char_insertion_and_keypress() {
    let (mut ctx, sid) = setup().await;
    evaluate(
        &mut ctx,
        10,
        "(function () {
            document.addEventListener('keydown', function (e) { if (e.key === 'x') e.preventDefault(); });
            document.getElementById('terminal-input').focus();
            return 'ok';
        })()",
        &sid,
    )
    .await;

    send_char(&mut ctx, 11, &sid, "x").await;
    send_char(&mut ctx, 12, &sid, "y").await;

    let value = evaluate(&mut ctx, 13, "document.getElementById('terminal-input').value", &sid).await;
    assert_eq!(value["result"]["value"].as_str(), Some("y"), "a prevented keydown must suppress insertion; the next char still types: {value}");

    let events = log(&mut ctx, 14, &sid).await;
    assert!(
        events.iter().any(|e| e == "keydown:x:t") && events.iter().any(|e| e == "keyup:x"),
        "prevented keydown still fires keydown and keyup: {events:?}"
    );
    assert!(
        !events.iter().any(|e| e == "keypress:x"),
        "a prevented keydown must not fire keypress: {events:?}"
    );
    assert!(
        events.iter().any(|e| e == "keypress:y"),
        "an allowed char still fires keypress: {events:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn label_click_focuses_its_control() {
    // A real browser focuses the labeled control when its label is clicked;
    // typed text must land in the field the label points at.
    let (mut ctx, sid) = setup().await;
    click_element(&mut ctx, 10, &sid, "#lbl").await;

    let active = evaluate(&mut ctx, 20, "document.activeElement && document.activeElement.id", &sid).await;
    assert_eq!(active["result"]["value"].as_str(), Some("field2"), "clicking a label focuses its control: {active}");

    send_char(&mut ctx, 21, &sid, "z").await;
    let value = evaluate(&mut ctx, 22, "document.getElementById('field2').value", &sid).await;
    assert_eq!(value["result"]["value"].as_str(), Some("z"), "typing goes to the labeled control: {value}");
}