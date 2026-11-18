//! Regression parity for issue #389: Cloudflare managed challenges hang because
//! bootstrap.js stubbed two structured-clone primitives the turnstile
//! orchestrate VM depends on:
//!
//!   1. `structuredClone` must preserve ArrayBuffer / TypedArray bytes (a JSON
//!      fallback serializes them to `{}`).
//!   2. A `CryptoKey` must survive `structuredClone` and remain usable by
//!      `crypto.subtle` (a WeakMap keyed by object identity leaves a clone
//!      without key material, so it throws "not a valid CryptoKey").
//!
//! Plus the follow-ups for DataView (#419 area), circular Error.cause (#419),
//! own `__proto__` properties (#420), non-cloneable values and CryptoKey
//! identity (#423).
//!
//! Ported from the telemaco-cdp suite to the Page API when the CDP server was
//! removed; the assertions are unchanged.

use std::io::{Read, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
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
        "structured-clone".to_string(),
        None,
        false,
        None,
        None,
        true,
    ));
    let mut page = Page::new("structured-clone-page".to_string(), context);
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
async fn structured_clone_preserves_arraybuffer_bytes() {
    let mut page = open().await;
    // A 4-byte view into a 4-byte buffer. The JSON fallback loses the buffer
    // entirely (Uint8Array serializes to {}), so byteLength reads back as 0.
    let val = eval_awaited_json(
        &mut page,
        r#"(async () => {
            const src = new Uint8Array([10, 20, 30, 40]);
            const clone = structuredClone(src);
            return JSON.stringify({
                srcLen: src.byteLength,
                cloneLen: clone.byteLength,
                same: src.buffer === clone.buffer,
                bytes: Array.from(clone),
            });
        })()"#,
    )
    .await;
    assert_eq!(val["srcLen"], 4);
    assert_eq!(val["cloneLen"], 4, "structuredClone dropped the ArrayBuffer");
    assert_eq!(val["same"], false, "clone must be independent, not the same buffer");
    assert_eq!(val["bytes"], json!([10, 20, 30, 40]));
}

#[tokio::test(flavor = "current_thread")]
async fn cryptokey_survives_structured_clone_and_still_signs() {
    let mut page = open().await;
    // importKey -> structuredClone -> sign with the clone. Without the fix the
    // clone has no WeakMap entry, so sign throws "Argument is not a valid CryptoKey".
    let val = eval_awaited_json(
        &mut page,
        r#"(async () => {
            const key = await crypto.subtle.importKey(
                "raw", new Uint8Array(32),
                { name: "HMAC", hash: "SHA-256" }, true, ["sign"]
            );
            const clone = structuredClone(key);
            const sig = await crypto.subtle.sign("HMAC", clone, new TextEncoder().encode("abc"));
            const b = new Uint8Array(sig);
            return JSON.stringify({
                cloneType: clone.type,
                cloneTag: clone[Symbol.toStringTag],
                sigLen: b.length,
            });
        })()"#,
    )
    .await;
    assert_eq!(val["cloneType"], "secret");
    assert_eq!(val["cloneTag"], "CryptoKey");
    assert_eq!(val["sigLen"], 32, "cloned CryptoKey must remain usable by crypto.subtle");
}

// DataView has no .slice() method, so the original TypedArray branch
// (`new Ctor(value.slice())`) threw `TypeError: value.slice is not a function`
// on every DataView clone.
#[tokio::test(flavor = "current_thread")]
async fn structured_clone_preserves_dataview() {
    let mut page = open().await;
    let val = eval_awaited_json(
        &mut page,
        r#"(async () => {
            const buf = new ArrayBuffer(8);
            const view = new DataView(buf);
            view.setUint32(0, 0x12345678);
            view.setUint32(4, 0x9abcdef0);
            const clone = structuredClone(view);
            return JSON.stringify({
                len: clone.byteLength,
                a: clone.getUint32(0),
                b: clone.getUint32(4),
                independent: clone.buffer !== view.buffer,
            });
        })()"#,
    )
    .await;
    assert_eq!(val["len"], 8, "DataView clone must keep its length");
    assert_eq!(val["a"], 0x12345678);
    assert_eq!(val["b"], 0x9abcdef0u64 as i64);
    assert_eq!(val["independent"], true, "clone must own its buffer, not alias the source");
}

// A reference cycle through Error.cause must clone without crashing (issue
// #419). Chrome clones this and preserves identity (clone.cause === clone).
#[tokio::test(flavor = "current_thread")]
async fn structured_clone_handles_circular_error_cause() {
    let mut page = open().await;
    let val = eval_awaited_json(
        &mut page,
        r#"(async () => {
            const e = new Error("boom");
            e.cause = e;
            const out = {};
            try {
                const clone = structuredClone(e);
                out.ok = true;
                out.message = clone.message;
                out.selfCycle = clone.cause === clone;
                out.isError = clone instanceof Error;
            } catch (err) {
                out.ok = false;
                out.err = String(err && err.message || err);
            }
            return JSON.stringify(out);
        })()"#,
    )
    .await;
    assert_eq!(val["ok"], true, "circular Error.cause crashed structuredClone: {:?}", val["err"]);
    assert_eq!(val["message"], "boom");
    assert_eq!(val["isError"], true, "clone must remain an Error");
    assert_eq!(val["selfCycle"], true, "cyclic cause must resolve to the clone, not a duplicate");
}

// An own enumerable `__proto__` data property (what JSON.parse('{"__proto__":…}')
// produces) must clone as an own data property, not be routed through the
// inherited __proto__ setter (issue #420). Plain objects must also clone onto
// Object.prototype, matching Chrome.
#[tokio::test(flavor = "current_thread")]
async fn structured_clone_reproduces_own_proto_property() {
    let mut page = open().await;
    let val = eval_awaited_json(
        &mut page,
        r#"(async () => {
            const src = JSON.parse('{"__proto__":{"polluted":true},"a":1}');
            const clone = structuredClone(src);
            return JSON.stringify({
                hasOwnProto: Object.prototype.hasOwnProperty.call(clone, "__proto__"),
                plainProto: Object.getPrototypeOf(clone) === Object.prototype,
                polluted: clone.polluted === true,
                a: clone.a,
            });
        })()"#,
    )
    .await;
    assert_eq!(val["hasOwnProto"], true, "own __proto__ data property was lost");
    assert_eq!(val["plainProto"], true, "plain object must clone onto Object.prototype");
    assert_eq!(val["polluted"], false, "clone prototype was reparented via the __proto__ setter");
    assert_eq!(val["a"], 1);
}

// Functions and symbols are not structured-cloneable: they must throw
// DataCloneError instead of passing through by reference.
#[tokio::test(flavor = "current_thread")]
async fn structured_clone_rejects_functions_and_symbols() {
    let mut page = open().await;
    let val = eval_awaited_json(
        &mut page,
        r#"(async () => {
            const out = {};
            try { structuredClone(function f(){}); out.fn = "cloned"; }
            catch (e) { out.fn = e instanceof DOMException ? e.name : "TypeError:" + e.message; }
            try { structuredClone(Symbol("s")); out.sym = "cloned"; }
            catch (e) { out.sym = e instanceof DOMException ? e.name : "TypeError:" + e.message; }
            return JSON.stringify(out);
        })()"#,
    )
    .await;
    assert_eq!(val["fn"], "DataCloneError", "functions must not clone");
    assert_eq!(val["sym"], "DataCloneError", "symbols must not clone");
}

// structuredClone preserves reference identity within one graph, including for
// platform objects cloned through a hook (issue #423).
#[tokio::test(flavor = "current_thread")]
async fn structured_clone_preserves_cryptokey_identity() {
    let mut page = open().await;
    let val = eval_awaited_json(
        &mut page,
        r#"(async () => {
            const key = await crypto.subtle.importKey(
                "raw", new Uint8Array(32),
                { name: "HMAC", hash: "SHA-256" }, true, ["sign"]
            );
            const clone = structuredClone({ a: key, b: key });
            // The shared clone must still be usable by crypto.subtle.
            const sig = await crypto.subtle.sign("HMAC", clone.a, new TextEncoder().encode("abc"));
            return JSON.stringify({
                shared: clone.a === clone.b,
                distinctFromSource: clone.a !== key,
                tag: clone.a[Symbol.toStringTag],
                sigLen: new Uint8Array(sig).length,
            });
        })()"#,
    )
    .await;
    assert_eq!(val["shared"], true, "one CryptoKey reached twice must clone to one shared object");
    assert_eq!(val["distinctFromSource"], true, "clone must not alias the source key");
    assert_eq!(val["tag"], "CryptoKey");
    assert_eq!(val["sigLen"], 32, "the shared clone must remain usable by crypto.subtle");
}
