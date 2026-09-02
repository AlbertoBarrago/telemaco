//! Embedded engine loop for the GUI: drives a `CdpContext` from window
//! commands and streams page frames back to the UI thread as RGBA buffers.
//!
//! The engine thread owns the `CdpContext` (and therefore the V8 isolate); the
//! UI thread only talks to it through two channels, so no page state is ever
//! shared across threads. Everything the window needs already exists as CDP
//! methods (navigation, history, trusted input, screenshots, viewport
//! emulation), so this layer stays thin and input behavior is exactly what
//! Puppeteer and Playwright observe over the wire.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use telemaco_cdp::dispatch::{dispatch, CdpContext};
use telemaco_cdp::types::CdpRequest;
use serde_json::{json, Value};

/// Automatic capture pace while the page is changing or the user is active
/// (mirrors the adaptive policy of tools/live-view.mjs, tightened for input).
const ACTIVE_CAPTURE_MS: u64 = 120;
/// Automatic capture pace for a settled page.
const IDLE_CAPTURE_MS: u64 = 450;
/// Consecutive unchanged frames before the capture pace backs off.
const IDLE_AFTER_TICKS: u32 = 4;
/// How long the command loop waits before re-checking the page.
const COMMAND_POLL_MS: u64 = 20;

/// Settings for the embedded engine thread.
#[derive(Debug, Default, Clone)]
pub struct EngineOptions {
    pub proxy: Option<String>,
    pub stealth: bool,
    pub user_agent: Option<String>,
    pub start_url: Option<String>,
}

/// UI -> engine commands. Input coordinates are CSS pixels relative to the
/// page viewport; wheel deltas follow native WheelEvent semantics (positive
/// scrolls down/right).
pub enum Command {
    Navigate { url: String },
    Back,
    Forward,
    Reload,
    MousePressed { x: f32, y: f32, button: &'static str, click_count: u32, modifiers: u32 },
    MouseReleased { x: f32, y: f32, button: &'static str, click_count: u32, modifiers: u32 },
    Wheel { x: f32, y: f32, dx: f32, dy: f32 },
    KeyDown { key: String, code: String, modifiers: u32 },
    KeyUp { key: String, code: String, modifiers: u32 },
    Char { text: String, modifiers: u32 },
    Viewport { width: u32, height: u32, scale: f32 },
    Shutdown,
}

/// Engine -> UI updates.
pub enum Update {
    Frame { rgba: Vec<u8>, width: u32, height: u32 },
    Status { url: String, title: String, loading: bool, can_back: bool, can_forward: bool },
    Error(String),
}

/// State shared by the engine's helpers across the loop.
struct EngineState {
    session: Option<String>,
    next_id: u64,
    loading: bool,
    can_back: bool,
    can_forward: bool,
}

/// Spawn the engine thread. Returns the command sender and the update
/// receiver; dropping the sender stops the engine.
pub fn spawn(options: EngineOptions) -> (Sender<Command>, Receiver<Update>) {
    let (command_tx, command_rx) = std::sync::mpsc::channel();
    let (update_tx, update_rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("telemaco-gui-engine".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = update_tx.send(Update::Error(format!("engine runtime: {error}")));
                    return;
                }
            };
            if let Err(error) = runtime.block_on(engine_loop(options, command_rx, update_tx.clone())) {
                let _ = update_tx.send(Update::Error(error));
            }
        })
        .expect("telemaco-gui: failed to spawn engine thread");
    (command_tx, update_rx)
}

async fn engine_loop(
    options: EngineOptions,
    command_rx: Receiver<Command>,
    updates: Sender<Update>,
) -> Result<(), String> {
    let mut ctx =
        CdpContext::new_with_full_options(options.proxy.clone(), options.stealth, options.user_agent.clone());
    let page_id = ctx.create_page();
    let session = format!("gui-{page_id}");
    ctx.sessions.insert(session.clone(), page_id);
    let mut state = EngineState {
        session: Some(session),
        next_id: 1,
        loading: false,
        can_back: false,
        can_forward: false,
    };

    if let Some(url) = options.start_url {
        if let Err(error) = navigate(&mut ctx, &mut state, &normalize_url(&url), &updates).await {
            let _ = updates.send(Update::Error(error));
        }
    } else {
        state.refresh_status(&mut ctx, &updates).await;
    }

    let mut last_frame: Option<(Vec<u8>, u32, u32)> = None;
    let mut last_generation: Option<u64> = None;
    // Saturating max so the first tick always captures.
    let mut unchanged_ticks: u32 = u32::MAX;
    let mut next_capture = Instant::now();

    loop {
        match command_rx.recv_timeout(Duration::from_millis(COMMAND_POLL_MS)) {
            Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
            Ok(command) => {
                if let Err(error) = apply_command(&mut ctx, &mut state, command, &updates).await {
                    let _ = updates.send(Update::Error(error));
                }
                // A queued interaction must land on screen quickly.
                next_capture = Instant::now() + Duration::from_millis(ACTIVE_CAPTURE_MS);
            }
            Err(RecvTimeoutError::Timeout) => {}
        }

        // Give timers and microtasks a slice so animated pages keep moving.
        if let Some(page) = ctx.get_session_page_mut(&state.session) {
            page.settle(8).await;
        }
        drain_events(&mut ctx, &mut state, &updates).await;

        let (generation, css_animating) = page_activity(&ctx, &state.session);
        if generation != last_generation || css_animating {
            unchanged_ticks = 0;
        }
        last_generation = generation;

        if Instant::now() >= next_capture {
            match capture(&mut ctx, &mut state, &mut last_frame, &updates).await {
                Ok(changed) => {
                    unchanged_ticks = if changed { 0 } else { unchanged_ticks.saturating_add(1) };
                }
                Err(error) => {
                    let _ = updates.send(Update::Error(error));
                    unchanged_ticks = 0;
                }
            }
            let pace = if state.loading || unchanged_ticks < IDLE_AFTER_TICKS {
                ACTIVE_CAPTURE_MS
            } else {
                IDLE_CAPTURE_MS
            };
            next_capture = Instant::now() + Duration::from_millis(pace);
        }
    }
    Ok(())
}

async fn apply_command(
    ctx: &mut CdpContext,
    state: &mut EngineState,
    command: Command,
    updates: &Sender<Update>,
) -> Result<(), String> {
    match command {
        Command::Shutdown => Ok(()),
        Command::Navigate { url } => navigate(ctx, state, &url, updates).await,
        Command::Back => go_history(ctx, state, -1, updates).await,
        Command::Forward => go_history(ctx, state, 1, updates).await,
        Command::Reload => {
            state.loading = true;
            cdp(ctx, state, "Page.reload", json!({})).await?;
            state.loading = false;
            state.refresh_status(ctx, updates).await;
            Ok(())
        }
        Command::MousePressed { x, y, button, click_count, modifiers } => {
            cdp(
                ctx,
                state,
                "Input.dispatchMouseEvent",
                json!({
                    "type": "mousePressed",
                    "x": x, "y": y,
                    "button": button,
                    "clickCount": click_count,
                    "modifiers": modifiers,
                }),
            )
            .await?;
            Ok(())
        }
        Command::MouseReleased { x, y, button, click_count, modifiers } => {
            cdp(
                ctx,
                state,
                "Input.dispatchMouseEvent",
                json!({
                    "type": "mouseReleased",
                    "x": x, "y": y,
                    "button": button,
                    "clickCount": click_count,
                    "modifiers": modifiers,
                }),
            )
            .await?;
            Ok(())
        }
        Command::Wheel { x, y, dx, dy } => {
            cdp(
                ctx,
                state,
                "Input.dispatchMouseEvent",
                json!({
                    "type": "mouseWheel",
                    "x": x, "y": y,
                    "deltaX": dx,
                    "deltaY": dy,
                }),
            )
            .await?;
            Ok(())
        }
        Command::KeyDown { key, code, modifiers } => {
            cdp(
                ctx,
                state,
                "Input.dispatchKeyEvent",
                json!({ "type": "keyDown", "modifiers": modifiers, "key": key, "code": code }),
            )
            .await?;
            Ok(())
        }
        Command::KeyUp { key, code, modifiers } => {
            cdp(
                ctx,
                state,
                "Input.dispatchKeyEvent",
                json!({ "type": "keyUp", "modifiers": modifiers, "key": key, "code": code }),
            )
            .await?;
            Ok(())
        }
        Command::Char { text, modifiers } => {
            cdp(
                ctx,
                state,
                "Input.dispatchKeyEvent",
                json!({ "type": "char", "text": text, "modifiers": modifiers }),
            )
            .await?;
            Ok(())
        }
        Command::Viewport { width, height, scale } => {
            cdp(
                ctx,
                state,
                "Emulation.setDeviceMetricsOverride",
                json!({
                    "width": width,
                    "height": height,
                    "deviceScaleFactor": scale,
                    "mobile": false,
                }),
            )
            .await?;
            Ok(())
        }
    }
}

async fn cdp(
    ctx: &mut CdpContext,
    state: &mut EngineState,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let request = CdpRequest {
        id: state.next_id,
        method: method.to_string(),
        params,
        session_id: state.session.clone(),
    };
    state.next_id += 1;
    let response = dispatch(&request, ctx).await;
    if let Some(error) = response.error {
        return Err(error.message);
    }
    Ok(response.result.unwrap_or(serde_json::Value::Null))
}

async fn navigate(
    ctx: &mut CdpContext,
    state: &mut EngineState,
    url: &str,
    updates: &Sender<Update>,
) -> Result<(), String> {
    state.loading = true;
    let _ = updates.send(Update::Status {
        url: url.to_string(),
        title: String::new(),
        loading: true,
        can_back: state.can_back,
        can_forward: state.can_forward,
    });
    let result = cdp(ctx, state, "Page.navigate", json!({ "url": url, "waitUntil": "load" })).await;
    state.loading = false;
    match result {
        Ok(_) => {
            state.refresh_status(ctx, updates).await;
            Ok(())
        }
        Err(error) => {
            state.refresh_status(ctx, updates).await;
            Err(format!("navigation to {url} failed: {error}"))
        }
    }
}
async fn go_history(
    ctx: &mut CdpContext,
    state: &mut EngineState,
    delta: i32,
    updates: &Sender<Update>,
) -> Result<(), String> {
    let history = cdp(ctx, state, "Page.getNavigationHistory", json!({})).await?;
    let index = history.get("currentIndex").and_then(Value::as_i64).unwrap_or(0);
    let entries = history
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let target = index + i64::from(delta);
    if target < 0 || target as usize >= entries.len() {
        return Ok(());
    }
    let entry_id = entries[target as usize]
        .get("id")
        .and_then(Value::as_u64)
        .unwrap_or(target as u64);
    state.loading = true;
    let result = cdp(ctx, state, "Page.navigateToHistoryEntry", json!({ "entryId": entry_id })).await;
    state.loading = false;
    if let Err(error) = result {
        return Err(format!("history navigation failed: {error}"));
    }
    state.refresh_status(ctx, updates).await;
    Ok(())
}
impl EngineState {
    async fn refresh_status(&mut self, ctx: &mut CdpContext, updates: &Sender<Update>) {
        let url = ctx
            .get_session_page(&self.session)
            .map(|page| page.url_string())
            .unwrap_or_else(|| "about:blank".to_string());
        let title = match cdp(
            ctx,
            self,
            "Runtime.evaluate",
            json!({ "expression": "document.title", "returnByValue": true }),
        )
        .await
        {
            Ok(value) => value
                .pointer("/result/value")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            Err(_) => String::new(),
        };
        if let Ok(history) = cdp(ctx, self, "Page.getNavigationHistory", json!({})).await {
            let index = history
                .get("currentIndex")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let len = history
                .get("entries")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0) as i64;
            self.can_back = index > 0;
            self.can_forward = index + 1 < len;
        }
        let _ = updates.send(Update::Status {
            url,
            title,
            loading: self.loading,
            can_back: self.can_back,
            can_forward: self.can_forward,
        });
    }
}
async fn capture(
    ctx: &mut CdpContext,
    state: &mut EngineState,
    last: &mut Option<(Vec<u8>, u32, u32)>,
    updates: &Sender<Update>,
) -> Result<bool, String> {
    let result = cdp(ctx, state, "Page.captureScreenshot", json!({ "format": "png" })).await?;
    let Some(data) = result.get("data").and_then(Value::as_str) else {
        return Err("Page.captureScreenshot returned no data".to_string());
    };
    let png = BASE64
        .decode(data)
        .map_err(|error| format!("screenshot base64 decode failed: {error}"))?;
    let decoded = image::load_from_memory(&png)
        .map_err(|error| format!("screenshot PNG decode failed: {error}"))?
        .to_rgba8();
    let (width, height) = decoded.dimensions();
    let rgba = decoded.into_raw();
    let changed = !matches!(
        last.as_ref(),
        Some(previous) if previous.1 == width && previous.2 == height && previous.0 == rgba
    );
    if changed {
        *last = Some((rgba.clone(), width, height));
        let _ = updates.send(Update::Frame { rgba, width, height });
    }
    Ok(changed)
}
async fn drain_events(ctx: &mut CdpContext, state: &mut EngineState, updates: &Sender<Update>) {
    if ctx.pending_events.is_empty() {
        return;
    }
    let events = std::mem::take(&mut ctx.pending_events);
    let mut changed = false;
    for event in events {
        if event.session_id != state.session {
            continue;
        }
        match event.method.as_str() {
            // A click or Enter submit navigates without an explicit Navigate
            // command; frameNavigated marks it, loadEventFired clears it.
            "Page.frameNavigated" => {
                state.loading = true;
                changed = true;
            }
            "Page.loadEventFired" => {
                state.loading = false;
                changed = true;
            }
            _ => {}
        }
    }
    if changed {
        state.refresh_status(ctx, updates).await;
    }
}
fn page_activity(ctx: &CdpContext, session: &Option<String>) -> (Option<u64>, bool) {
    let Some(page) = ctx.get_session_page(session) else {
        return (None, false);
    };
    let generation = page.js.as_ref().map(|js| js.activity_generation());
    let animating = page.prepared_has_active_css_animations();
    (generation, animating)
}

/// Turn omnibar input into a URL: pass schemes through, promote bare hosts
/// (localhost stays on http), and fall back to a search query for prose.
pub fn normalize_url(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return "about:blank".to_string();
    }
    let lower = trimmed.to_ascii_lowercase();
    for prefix in ["http://", "https://", "about:", "data:", "file:", "view-source:"] {
        if lower.starts_with(prefix) {
            return trimmed.to_string();
        }
    }
    let looks_like_host = !trimmed.contains(' ')
        && !trimmed.contains('\t')
        && (trimmed.contains('.') || lower.starts_with("localhost") || lower.starts_with("127.0.0.1"));
    if looks_like_host {
        if lower.starts_with("localhost") || lower.starts_with("127.0.0.1") {
            format!("http://{trimmed}")
        } else {
            format!("https://{trimmed}")
        }
    } else {
        format!("https://duckduckgo.com/?q={}", percent_encode_query(trimmed))
    }
}

/// Minimal percent-encoding for a search query component.
fn percent_encode_query(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_url_passes_through_schemes() {
        assert_eq!(normalize_url("https://example.com/x"), "https://example.com/x");
        assert_eq!(normalize_url("about:blank"), "about:blank");
        assert_eq!(normalize_url("data:text/html,hi"), "data:text/html,hi");
        assert_eq!(normalize_url("file:///tmp/a.html"), "file:///tmp/a.html");
    }

    #[test]
    fn normalize_url_promotes_bare_hosts() {
        assert_eq!(normalize_url("example.com"), "https://example.com");
        assert_eq!(normalize_url("example.com/path?q=1"), "https://example.com/path?q=1");
        assert_eq!(normalize_url("localhost:9222/status"), "http://localhost:9222/status");
        assert_eq!(normalize_url("127.0.0.1:8000"), "http://127.0.0.1:8000");
    }

    #[test]
    fn normalize_url_searches_prose() {
        assert_eq!(
            normalize_url("rust headless browser"),
            "https://duckduckgo.com/?q=rust%20headless%20browser"
        );
        assert_eq!(normalize_url(""), "about:blank");
        assert_eq!(normalize_url("   "), "about:blank");
    }
}