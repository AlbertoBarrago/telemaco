//! A submit button whose click listener calls `preventDefault()` and then
//! navigates by assigning `location.href` must leave the page on the target
//! URL with the form's fields in the query string.
//!
//! Ported from the telemaco-cdp suite (`cdp_click_submit_parity`) to the Page
//! API when the CDP server was removed; the assertions are unchanged. The CDP
//! suite clicked through `Runtime.callFunctionOn`, which runs the pending
//! JS navigation afterwards; here `process_pending_navigation` does that,
//! exactly as the MCP click tools do.

use std::io::{Read, Write};
use std::sync::Arc;

use serde_json::json;
use telemaco_browser::{BrowserContext, Page};

const FORM_PAGE: &str = r#"<html><body>
<form id="f" action="/submitted">
  <input type="hidden" name="vacancy_id" value="123">
  <textarea name="message">hello</textarea>
  <input type="checkbox" name="agree" value="yes" checked>
  <button id="submit" type="submit">Go</button>
</form>
<script>
function submitCompat() {
  const form = document.getElementById('f');
  const params = new URLSearchParams();
  form.querySelectorAll('input, textarea').forEach(function(field) {
    if (field.type === 'checkbox' && !field.checked) return;
    params.append(field.name, field.value);
  });
  location.href = form.action + '?' + params.toString();
}
document.querySelector('button').addEventListener('click', function(e) {
  e.preventDefault();
  submitCompat();
});
</script>
</body></html>"#;

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
                let body = if request.starts_with("GET /submitted") {
                    "<html><body>submitted</body></html>"
                } else {
                    FORM_PAGE
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            });
        }
    });
    format!("http://{address}/")
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_click_submit_prevent_default_navigation_updates_page() {
    std::env::set_var("TELEMACO_ALLOW_PRIVATE_NETWORK", "1");
    let url = serve();
    let context = Arc::new(BrowserContext::with_storage_and_network(
        "click-submit".to_string(),
        None,
        false,
        None,
        None,
        true,
    ));
    let mut page = Page::new("click-submit-page".to_string(), context);
    // Page::navigate waits for load, so the inline script defining
    // submitCompat has run.
    page.navigate(&url).await.unwrap();

    assert_eq!(page.evaluate("typeof submitCompat"), json!("function"));

    page.evaluate("document.getElementById('submit').click()");
    page.process_pending_navigation().await.unwrap();

    assert_eq!(page.url.as_ref().unwrap().path(), "/submitted");
    assert_eq!(
        page.url.as_ref().unwrap().query(),
        Some("vacancy_id=123&message=hello&agree=yes")
    );
    assert!(page
        .evaluate("document.body.textContent")
        .as_str()
        .unwrap()
        .contains("submitted"));
}
