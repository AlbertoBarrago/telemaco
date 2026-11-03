use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

// Run V8 in a separate process: a Tokio timeout cannot interrupt a synchronous
// loop on the server thread. The parent's socket deadline and Drop guard can.
#[test]
#[ignore = "subprocess fixture for evaluation_timeout_and_recovery"]
fn evaluation_timeout_server() {
    let Some(port) = std::env::var("TELEMACO_EVALUATION_TEST_PORT").ok() else {
        return;
    };
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
        .block_on(telemaco_mcp::http::run(
            "127.0.0.1".to_string(), port.parse().unwrap(), None, None,
            false, Default::default(), false,
        )).unwrap();
}

fn call(port: u16, name: &str, arguments: Value) -> Value {
    let body = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": name, "arguments": arguments },
    }).to_string();
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    stream.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
    write!(stream,
        "POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len(),
    ).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("MCP must reply within the external deadline");
    let (_, body) = response.split_once("\r\n\r\n").expect("HTTP response");
    serde_json::from_str::<Value>(body).expect("JSON-RPC response")["result"].clone()
}

#[test]
fn evaluation_timeout_and_recovery() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut server = Server(Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "evaluation_timeout_server"])
        .env("TELEMACO_EVALUATION_TEST_PORT", port.to_string())
        .env("TELEMACO_NAV_TIMEOUT_MS", "100")
        .stdout(Stdio::null())
        .spawn().unwrap());
    let ready_deadline = Instant::now() + Duration::from_secs(3);
    loop {
        assert!(server.0.try_wait().unwrap().is_none(), "server exited before binding");
        if TcpStream::connect(("127.0.0.1", port)).is_ok() { break; }
        assert!(Instant::now() < ready_deadline, "server failed to bind");
        std::thread::sleep(Duration::from_millis(10));
    }

    let navigate = call(port, "browser_navigate", json!({
        "url": "data:text/html,<html><body><canvas id=board width=64 height=48></canvas></body></html>",
    }));
    assert_ne!(navigate["isError"], true, "{navigate}");

    let null = call(port, "browser_evaluate", json!({"expression": "null"}));
    assert_ne!(null["isError"], true, "{null}");
    assert_eq!(null["content"][0]["text"], "null");

    let error = call(port, "browser_evaluate", json!({
        "expression": "(function(){throw new Error('evaluation failure')})()",
    }));
    assert_eq!(error["isError"], true, "{error}");
    assert!(error["content"][0]["text"].as_str().unwrap().contains("evaluation failure"));

    for budget in [json!(0), json!(-1), json!(31), json!("1"), Value::Null, json!(1e300)] {
        let invalid = call(port, "browser_evaluate", json!({
            "expression": "globalThis.shouldNotRun = true", "timeout": budget,
        }));
        assert_eq!(invalid["isError"], true, "{invalid}");
        assert!(invalid["content"][0]["text"].as_str().unwrap().contains("timeout must"));
    }
    let untouched = call(port, "browser_evaluate", json!({"expression": "typeof shouldNotRun"}));
    assert_eq!(untouched["content"][0]["text"], "undefined");

    const RESULT_CAP: usize = 1024 * 1024;
    let at_limit = call(port, "browser_evaluate", json!({
        "expression": format!("'x'.repeat({RESULT_CAP})"),
    }));
    assert_ne!(at_limit["isError"], true, "at-limit direct string must succeed");
    let at_limit_text = at_limit["content"][0]["text"].as_str().unwrap();
    assert_eq!(at_limit_text.len(), RESULT_CAP);
    assert!(at_limit_text.bytes().all(|byte| byte == b'x'));

    let multibyte_at_limit = call(port, "browser_evaluate", json!({
        "expression": format!("'é'.repeat({})", RESULT_CAP / 2),
    }));
    assert_ne!(multibyte_at_limit["isError"], true, "at-limit multibyte string must succeed");
    assert_eq!(
        multibyte_at_limit["content"][0]["text"].as_str().unwrap().len(),
        RESULT_CAP,
    );

    let object_at_limit = call(port, "browser_evaluate", json!({
        "expression": format!("({{data:'x'.repeat({})}})", RESULT_CAP - 16),
    }));
    assert_ne!(object_at_limit["isError"], true, "at-limit formatted object must succeed");
    assert_eq!(object_at_limit["content"][0]["text"].as_str().unwrap().len(), RESULT_CAP);

    for (label, expression) in [
        (
            "direct string above the byte cap",
            format!("'x'.repeat({})", RESULT_CAP + 1),
        ),
        (
            "multibyte string above the byte cap",
            format!("'é'.repeat({})", RESULT_CAP / 2 + 1),
        ),
        (
            "object compact JSON above the byte cap",
            format!("({{data:'x'.repeat({RESULT_CAP})}})"),
        ),
        (
            "pretty output above the byte cap",
            "Array(180000).fill('x')".to_string(),
        ),
        (
            "custom stringify non-string conversion above the byte cap",
            format!(
                "(function(){{const original=JSON.stringify;JSON.stringify=function(){{try{{return {{toString(){{return 'x'.repeat({})}}}}}}finally{{JSON.stringify=original}}}};return {{value:1}}}})()",
                RESULT_CAP + 1,
            ),
        ),
        (
            "fallback object conversion above the byte cap",
            format!(
                "(function(){{const original=JSON.stringify;JSON.stringify=function(){{try{{return undefined}}finally{{JSON.stringify=original}}}};return {{toString(){{return 'x'.repeat({})}}}}}})()",
                RESULT_CAP + 1,
            ),
        ),
    ] {
        let result = call(port, "browser_evaluate", json!({"expression": expression}));
        assert_eq!(result["isError"], true, "{label}");
        assert!(result["content"][0]["text"].as_str().unwrap().contains("byte limit"), "{label}");

        let recovered = call(port, "browser_evaluate", json!({"expression": "1+1"}));
        assert_ne!(recovered["isError"], true, "recovery after {label}");
        assert_eq!(recovered["content"][0]["text"], "2.0");
    }

    // Repeated termination must preserve valid null results and permit later
    // JavaScript, DOM access, drawing, navigation and capture on this session.
    for expression in [
        "(function(){ document.body.dataset.started = 'yes'; while(true){} })()",
        "({toJSON(){while(true){}}})",
        "({get value(){while(true){}}})",
        "new Proxy({}, {ownKeys(){while(true){}}})",
    ] {
        let start = Instant::now();
        let timed_out = call(port, "browser_evaluate", json!({
            "expression": expression,
            "timeout": 0.05,
        }));
        assert!(start.elapsed() < Duration::from_secs(2));
        assert_eq!(timed_out["isError"], true, "{timed_out}");
        assert!(timed_out["content"][0]["text"].as_str().unwrap().contains("timed out"));
        let recovered = call(port, "browser_evaluate", json!({
            "expression": "document.body.dataset.started === 'yes' ? 1+1 : 0",
        }));
        assert_ne!(recovered["isError"], true, "{recovered}");
        assert_eq!(recovered["content"][0]["text"], "2.0");
    }

    let draw = call(port, "browser_evaluate", json!({
        "expression": "(function(){const ctx=document.getElementById('board').getContext('2d');ctx.fillStyle='red';ctx.fillRect(0,0,64,48);return Array.from(ctx.getImageData(0,0,1,1).data)})()",
    }));
    assert_ne!(draw["isError"], true, "{draw}");
    assert_eq!(serde_json::from_str::<Value>(draw["content"][0]["text"].as_str().unwrap()).unwrap(), json!([255,0,0,255]));
    #[cfg(feature = "render")]
    {
        let capture = call(port, "browser_screenshot", json!({"width":64,"height":48}));
        assert_ne!(capture["isError"], true, "{capture}");
        assert_eq!(capture["content"][0]["type"], "image");
        assert_eq!(capture["content"][0]["mimeType"], "image/png");
        assert!(capture["content"][0]["data"].as_str().unwrap().starts_with("iVBOR"));
    }

    let navigation_start = Instant::now();
    call(port, "browser_navigate", json!({
        "url": "data:text/html,<html><body>next<script>while(true){}</script></body></html>",
    }));
    assert!(navigation_start.elapsed() < Duration::from_secs(2));
    let recovered = call(port, "browser_evaluate", json!({"expression":"1+1"}));
    assert_ne!(recovered["isError"], true, "{recovered}");
    assert_eq!(recovered["content"][0]["text"], "2.0");
}
