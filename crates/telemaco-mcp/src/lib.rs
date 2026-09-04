// The tools/list JSON literal expanded by serde_json::json! is large
// enough now (32 tool definitions) that the default macro recursion
// limit (128) overflows. Bumping for this crate only.
#![recursion_limit = "512"]

pub mod blocks;
pub mod config;
pub mod http;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
#[cfg(feature = "render")]
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use telemaco_browser::{BrowserContext, Page};
use telemaco_dom::NodeId;
use serde::{Deserialize, Serialize};

use crate::config::ExtractionLimits;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Deserialize)]
struct RpcMessage {
    #[allow(dead_code)]
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
struct RpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

impl RpcResponse {
    fn ok(id: Value, result: Value) -> Self {
        RpcResponse { jsonrpc: "2.0", id, result: Some(result), error: None }
    }

    fn err(id: Value, code: i32, message: impl Into<String>) -> Self {
        RpcResponse { jsonrpc: "2.0", id, result: None, error: Some(RpcError { code, message: message.into() }) }
    }
}

pub struct BrowserState {
    /// Open tabs keyed by tab_id (e.g. "tab-1"). BTreeMap so list ordering
    /// is stable across calls (agents reason about "tab #2" deterministically).
    tabs: std::collections::BTreeMap<String, Page>,
    /// The tab id every tool call operates on. None means there are no
    /// open tabs; the next page_mut() call creates one.
    active_tab: Option<String>,
    tab_counter: u32,
    context: Arc<BrowserContext>,
    user_agent: Option<String>,
    console_messages: Vec<String>,
    /// Element-ref table from the last `browser_snapshot` on the ACTIVE
    /// tab. Agents click / fill / type by `ref` (e.g. `"e3"`) instead of
    /// guessing a CSS selector. Refs are stable within a snapshot; the
    /// table is wiped on every navigation / tab switch and refilled on
    /// the next snapshot call.
    interactive_refs: HashMap<String, NodeId>,
    /// Output caps for every tool that can emit unbounded text. Resolved once
    /// at startup from config file, environment, and CLI flag; a tool argument
    /// still overrides per call. See [`crate::config`].
    limits: ExtractionLimits,
    /// Full-page markdown from the last `browser_markdown` conversion,
    /// keyed by page URL, so `page=N` requests slice a cached document
    /// instead of re-running the V8 converter on every call. Cleared on
    /// navigation; `refresh: true` forces re-conversion.
    markdown_cache: Option<(String, String)>,
}

impl BrowserState {
    pub fn new(
        proxy: Option<String>,
        user_agent: Option<String>,
        stealth: bool,
        limits: ExtractionLimits,
    ) -> Self {
        BrowserState {
            tabs: std::collections::BTreeMap::new(),
            active_tab: None,
            tab_counter: 0,
            context: Arc::new(BrowserContext::with_options("mcp".to_string(), proxy, stealth)),
            user_agent,
            console_messages: Vec::new(),
            interactive_refs: HashMap::new(),
            limits,
            markdown_cache: None,
        }
    }

    /// The resolved output caps for this server.
    pub fn limits(&self) -> &ExtractionLimits {
        &self.limits
    }

    /// Make sure there is at least one tab and return a &mut to the
    /// active tab's Page. Auto-creates a default tab if none exist so
    /// every legacy single-page tool continues to work without
    /// requiring an explicit browser_tab_new.
    fn page_mut(&mut self) -> &mut Page {
        if self.active_tab.is_none() {
            self.tab_counter += 1;
            let id = format!("tab-{}", self.tab_counter);
            self.tabs.insert(id.clone(), Page::new("mcp-page".to_string(), self.context.clone()));
            self.active_tab = Some(id);
        }
        let id = self.active_tab.as_ref().unwrap().clone();
        self.activate(&id);
        self.tabs.get_mut(&id).expect("active tab must exist")
    }

    fn new_tab(&mut self) -> String {
        self.tab_counter += 1;
        let id = format!("tab-{}", self.tab_counter);
        self.tabs.insert(id.clone(), Page::new(format!("mcp-{id}"), self.context.clone()));
        self.active_tab = Some(id.clone());
        self.interactive_refs.clear();
        id
    }

    /// Enforce the single-live-isolate invariant. rusty_v8 enters each V8
    /// isolate on creation and requires isolates be dropped in reverse order of
    /// creation, so keeping more than one tab's isolate live at once and then
    /// dropping a non-newest one aborts the whole process (#258). Suspend every
    /// other tab (drops its isolate, keeps its DOM in self.dom) and make the
    /// active tab the only live isolate, mirroring the CDP server's
    /// Dispatcher::get_session_page_mut.
    fn activate(&mut self, tab_id: &str) {
        for (id, page) in self.tabs.iter_mut() {
            if id.as_str() != tab_id && page.has_js() {
                page.suspend_js();
            }
        }
        if let Some(page) = self.tabs.get_mut(tab_id) {
            page.resume_js();
        }
    }

    /// Close a tab without breaking the LIFO isolate-drop rule. suspend_js drops
    /// this tab's isolate (if it is the live one) while it is still the only
    /// entered isolate, so the following remove disposes no isolate (#258).
    fn close_tab(&mut self, tab_id: &str) -> bool {
        if let Some(page) = self.tabs.get_mut(tab_id) {
            page.suspend_js();
        }
        self.tabs.remove(tab_id).is_some()
    }

    fn has_active_page_runtime(&self) -> bool {
        self.active_tab
            .as_ref()
            .and_then(|tab_id| self.tabs.get(tab_id))
            .is_some_and(Page::has_js)
    }

    /// Advance the active page by one wake-driven browser task and immediately
    /// consume any navigation that task queued. MCP owns its pages continuously,
    /// so leaving either half for the next tool call strands timers, fetches,
    /// and location/form/click navigations while the transport waits on stdin.
    async fn advance_active_page_tasks(&mut self) -> Result<bool, String> {
        let page = self.page_mut();
        let reached_idle = page.run_autonomous_event_loop_turn().await?;
        let navigated = page
            .process_pending_navigation()
            .await
            .map_err(|error| error.to_string())?;
        if navigated {
            self.interactive_refs.clear();
        }
        Ok(reached_idle && !navigated)
    }

    /// Consume a navigation that a synthesized interaction just queued,
    /// before the tool replies.
    ///
    /// A click on a submit button, an Enter keypress, or an evaluated
    /// `location.href` assignment does not issue a request by itself. It runs
    /// the page's own glue and leaves the navigation as pending state, which
    /// becomes a request only when a driving layer converts it. The CDP path
    /// already converts it, in `Input.dispatchMouseEvent` and after
    /// `Runtime.evaluate`, which is why the same click POSTs over CDP and
    /// does nothing over MCP (#618).
    ///
    /// [`Self::advance_active_page_tasks`] cannot cover this. It is armed
    /// after every dispatch, but it sits in a `biased` select behind
    /// `read_line`, so a client that sends its next tool call immediately,
    /// which an agent does, wins that race every time. The reply is also
    /// written before any pump turn could run, so the tool would answer
    /// "Clicked" while the request has not left the process.
    async fn settle_synthetic_navigation(&mut self) -> Result<(), String> {
        let navigated = self
            .page_mut()
            .process_pending_navigation()
            .await
            .map_err(|error| error.to_string())?;
        if navigated {
            // The ref table names elements in a document that has gone away.
            self.interactive_refs.clear();
            // One slice on the landed document, so a tool that reads the URL
            // or the text next sees the new page rather than an empty one.
            self.page_mut()
                .run_autonomous_event_loop_turn()
                .await
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    /// Resolve `ref=eN` to a CSS selector that uniquely targets the
    /// element. Snapshot writes `data-telemaco-ref="eN"` onto every
    /// interactable, so the attribute survives across calls as long as
    /// the page isn't re-rendered without it. Returns `Err` if the ref
    /// hasn't been registered (caller must call browser_snapshot first).
    fn ref_to_selector(&self, r: &str) -> Result<String, String> {
        if !self.interactive_refs.contains_key(r) {
            return Err(format!(
                "unknown ref '{r}'; call browser_snapshot first to refresh the ref table"
            ));
        }
        Ok(format!("[data-telemaco-ref=\"{r}\"]"))
    }
}

pub(crate) async fn dispatch(method: &str, id: Value, params: &Value, state: &mut BrowserState) -> RpcResponse {
    match method {
        "initialize" => handle_initialize(id, params),
        "ping" => RpcResponse::ok(id, json!({})),
        "tools/list" => handle_tools_list(id, state.limits()),
        "tools/call" => handle_tool_call(id, params, state).await,
        "resources/list" => RpcResponse::ok(id, json!({"resources": []})),
        "prompts/list" => RpcResponse::ok(id, json!({"prompts": []})),
        _ => RpcResponse::err(id, -32601, format!("Unknown method: {method}")),
    }
}

pub async fn run(
    proxy: Option<String>,
    user_agent: Option<String>,
    stealth: bool,
    limits: ExtractionLimits,
) -> Result<()> {
    // A stdio MCP server reads JSON-RPC from stdin and answers on stdout, so
    // with no client attached it correctly sits silent. Typed at a prompt that
    // is indistinguishable from a hang, so say what is happening. stderr only:
    // anything on stdout would corrupt the protocol stream, while clients
    // ignore stderr.
    if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        eprintln!(
            "telemaco mcp: MCP server on stdio, waiting for JSON-RPC on stdin.\n\
             \n\
             This command is meant to be launched by an MCP client, not run by hand.\n\
             Point your client at it, for example Claude Code:\n\
             \n\
             \x20   claude mcp add telemaco -- telemaco mcp\n\
             \n\
             To try it here, pipe a request in:\n\
             \n\
             \x20   echo '{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}}' | telemaco mcp\n\
             \n\
             For an HTTP endpoint instead, use --http. Ctrl-C to quit."
        );
    }

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    let mut writer = stdout;

    let mut state = BrowserState::new(proxy, user_agent, stealth, limits);
    let mut runtime_pump_armed = false;

    loop {
        // MCP stdio transport: newline-delimited JSON (one message per line)
        let mut line = String::new();
        let n = if runtime_pump_armed {
            tokio::select! {
                biased;
                read = reader.read_line(&mut line) => Some(read?),
                pump_result = state.advance_active_page_tasks() => {
                    match pump_result {
                        Ok(reached_idle) => runtime_pump_armed = !reached_idle,
                        Err(error) => {
                            runtime_pump_armed = false;
                            eprintln!("MCP page task failed: {error}");
                        }
                    }
                    None
                }
            }
        } else {
            Some(reader.read_line(&mut line).await?)
        };
        let Some(n) = n else {
            continue;
        };
        if n == 0 {
            return Ok(());
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let msg: RpcMessage = match serde_json::from_str(trimmed) {
            Ok(m) => m,
            Err(_) => continue,
        };

        // Notifications (no id) need no response
        if msg.id.is_none() {
            continue;
        }

        let id = msg.id.clone().unwrap_or(Value::Null);
        let response = dispatch(&msg.method, id, &msg.params, &mut state).await;
        runtime_pump_armed = state.has_active_page_runtime();

        let mut body = serde_json::to_string(&response)?;
        body.push('\n');
        writer.write_all(body.as_bytes()).await?;
        writer.flush().await?;
    }
}

fn handle_initialize(id: Value, params: &Value) -> RpcResponse {
    let _client_version = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
    RpcResponse::ok(id, json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": "telemaco-mcp",
            "version": env!("CARGO_PKG_VERSION")
        }
    }))
}

/// Advertise the tool schemas.
///
/// The per-tool default shown in each description is the RESOLVED value for
/// this server, not a hardcoded literal: once the caps became configurable, a
/// fixed string in the schema would misreport the server's own behavior to the
/// client.
fn handle_tools_list(id: Value, limits: &ExtractionLimits) -> RpcResponse {
    // `0` means unlimited; say so rather than printing the sentinel.
    let shown = |value: usize| -> String {
        if value == 0 {
            "unlimited".to_string()
        } else {
            value.to_string()
        }
    };
    let max_chars = shown(limits.max_chars);
    let max_links = shown(limits.max_links);
    let max_interactive = shown(limits.max_interactive);
    let max_search_results = shown(limits.max_search_results);
    let search_context_chars = shown(limits.search_context_chars);
    #[allow(unused_mut)]
    let mut tools = json!([
            {
                "name": "browser_navigate",
                "description": "Navigate to a URL and wait for the page to load",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "url": { "type": "string", "description": "URL to navigate to" },
                        "waitUntil": {
                            "type": "string",
                            "enum": ["load", "domcontentloaded", "networkidle0"],
                            "description": "Navigation wait condition (default: load)"
                        }
                    },
                    "required": ["url"]
                }
            },
            {
                "name": "browser_snapshot",
                "description": "Get the current page content as text (title, URL, and readable body text)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "max_chars": { "type": "number", "minimum": 0, "description": format!("Truncate readable body text to this many characters (default: {max_chars})") }
                    },
                    "additionalProperties": false
                }
            },
            {
                "name": "browser_click",
                "description": "Click an element. Pass `ref` (preferred, from browser_snapshot / browser_interactive_elements) OR a `selector`.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "ref": { "type": "string", "description": "Element ref like 'e3' from a recent snapshot" },
                        "selector": { "type": "string", "description": "CSS selector (fallback if ref unavailable)" }
                    }
                }
            },
            {
                "name": "browser_fill",
                "description": "Set the value of an input element. Pass `ref` (preferred) OR `selector`.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "ref": { "type": "string" },
                        "selector": { "type": "string" },
                        "value": { "type": "string", "description": "Value to set" }
                    },
                    "required": ["value"]
                }
            },
            {
                "name": "browser_type",
                "description": "Type text into an input element (appends to existing value). Pass `ref` (preferred) OR `selector`.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "ref": { "type": "string" },
                        "selector": { "type": "string" },
                        "text": { "type": "string", "description": "Text to type" }
                    },
                    "required": ["text"]
                }
            },
            {
                "name": "browser_press_key",
                "description": "Dispatch a keyboard event on an element or the document",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "key": { "type": "string", "description": "Key name (e.g. Enter, Tab, Escape)" },
                        "selector": { "type": "string", "description": "CSS selector (optional, defaults to document)" }
                    },
                    "required": ["key"]
                }
            },
            {
                "name": "browser_select_option",
                "description": "Select an option from a <select> element",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "selector": { "type": "string", "description": "CSS selector of the <select> element" },
                        "value": { "type": "string", "description": "Value or text of the option to select" }
                    },
                    "required": ["selector", "value"]
                }
            },
            {
                "name": "browser_evaluate",
                "description": "Evaluate a JavaScript expression in the page context and return the result",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "expression": { "type": "string", "description": "JavaScript expression to evaluate" }
                    },
                    "required": ["expression"]
                }
            },
            {
                "name": "browser_wait_for",
                "description": "Wait for a CSS selector to appear in the DOM",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "selector": { "type": "string", "description": "CSS selector to wait for" },
                        "timeout": { "type": "number", "description": "Timeout in seconds (default: 30)" }
                    },
                    "required": ["selector"]
                }
            },
            {
                "name": "browser_network_requests",
                "description": "Return the list of network requests made by the current page",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "browser_console_messages",
                "description": "Return the console messages logged by the current page",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "browser_close",
                "description": "Close the current browser page and reset state",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "browser_markdown",
                "description": "Extract the current page as Markdown (headings, paragraphs, lists, links, code blocks). Use this instead of browser_snapshot when you want token-dense structured content rather than plain text. The document is paginated at block boundaries: the first call converts the page once and returns page 1; pass page=2, 3, ... to read further chunks without re-converting.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "max_chars": { "type": "number", "description": format!("Max characters per page (default {max_chars}). Blocks are never cut in half; an oversized block is split across pages.") },
                        "page": { "type": "number", "description": "1-based page to read. Default 1. Pages beyond the end return an explicit out-of-range message." },
                        "refresh": { "type": "boolean", "description": "Force re-conversion of the current page instead of serving the cached markdown" }
                    }
                }
            },
            {
                "name": "browser_links",
                "description": "List every anchor link on the current page as one JSON object per line: {text, href}. Use when you need to enumerate where to navigate next.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "limit": { "type": "number", "description": format!("Max number of links to return (default {max_links})") },
                        "internal_only": { "type": "boolean", "description": "If true, only return links on the same origin as the current page" }
                    }
                }
            },
            {
                "name": "browser_interactive_elements",
                "description": "List every clickable / typeable element on the current page with a stable ref ID and a brief description. Use this BEFORE clicking or filling so you can refer to elements by ref instead of guessing a CSS selector. Refs look like 'e3' and stay valid until the next navigation.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "limit": { "type": "number", "description": format!("Max number of elements (default {max_interactive})") }
                    }
                }
            },
            {
                "name": "browser_back",
                "description": "Navigate back in the page history (equivalent to the browser back button).",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "browser_forward",
                "description": "Navigate forward in the page history.",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "browser_reload",
                "description": "Reload the current page.",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "browser_get_cookies",
                "description": "Return all cookies in the browser's cookie jar as one JSON object per line.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "domain": { "type": "string", "description": "Filter to cookies on this domain (default: all)" }
                    }
                }
            },
            {
                "name": "browser_set_cookie",
                "description": "Add or replace a cookie in the jar. Use this to skip a login flow when you already have a session token.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "value": { "type": "string" },
                        "domain": { "type": "string", "description": "e.g. example.com or .example.com" },
                        "path": { "type": "string", "description": "default '/'" },
                        "secure": { "type": "boolean" },
                        "http_only": { "type": "boolean" }
                    },
                    "required": ["name", "value", "domain"]
                }
            },
            {
                "name": "browser_clear_cookies",
                "description": "Wipe every cookie from the jar.",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "browser_wait_for_text",
                "description": "Wait until a substring appears anywhere in the rendered page text. Use when you want to wait for a result message or notification rather than a specific selector.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "text": { "type": "string" },
                        "timeout": { "type": "number", "description": "Seconds (default 30)" }
                    },
                    "required": ["text"]
                }
            },
            {
                "name": "browser_detect_forms",
                "description": "List every <form> on the page with its action URL, method, and a description of each input/textarea/select. Use to understand a form's structure before filling it in.",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "browser_fill_form",
                "description": "Fill multiple inputs in one call. `fields` is an array of {ref?, selector?, value, type?}. type='text' (default) sets value, type='check'/'uncheck' toggles checkboxes, type='select' picks an option by value or visible text. Saves N round-trips vs N browser_fill calls.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "fields": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "ref": { "type": "string" },
                                    "selector": { "type": "string" },
                                    "value": { "type": "string" },
                                    "type": { "type": "string", "enum": ["text", "check", "uncheck", "select"] }
                                }
                            }
                        },
                        "submit_ref": { "type": "string", "description": "Optional: click this element after filling (e.g. submit button ref)" },
                        "submit_selector": { "type": "string" }
                    },
                    "required": ["fields"]
                }
            },
            {
                "name": "browser_scroll",
                "description": "Scroll the page or an element. `direction` is 'top'|'bottom'|'up'|'down'|'left'|'right' (default 'down'). `amount` in pixels (default viewport height). Use 'bottom' to trigger infinite-scroll loaders.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "direction": { "type": "string", "enum": ["top", "bottom", "up", "down", "left", "right"] },
                        "amount": { "type": "number", "description": "Pixels (default: one viewport)" },
                        "ref": { "type": "string", "description": "Optional element to scroll into view" },
                        "selector": { "type": "string" }
                    }
                }
            },
            {
                "name": "browser_get_attribute",
                "description": "Read an attribute of an element (href, src, value, class, data-*, etc.). Returns the raw attribute value as a string, or empty string if missing.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "ref": { "type": "string" },
                        "selector": { "type": "string" },
                        "attribute": { "type": "string", "description": "Attribute name (e.g. href, value, src)" }
                    },
                    "required": ["attribute"]
                }
            },
            {
                "name": "browser_count",
                "description": "Count how many elements on the page match a CSS selector. Cheap existence / pagination probe.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "selector": { "type": "string" }
                    },
                    "required": ["selector"]
                }
            },
            {
                "name": "browser_extract",
                "description": "Extract a structured object from the page given a map of {field_name: css_selector}. Returns one JSON object with each field set to the matching element's text content (or attribute via 'selector@attr' syntax, e.g. 'a@href'). For list extraction, append '[]' to the field name (e.g. 'rows[]') and the value will be an array.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "schema": {
                            "type": "object",
                            "description": "Map of field_name to CSS selector. Suffix selector with '@attr' for attribute, suffix field name with '[]' for array."
                        }
                    },
                    "required": ["schema"]
                }
            },
            {
                "name": "browser_tab_new",
                "description": "Open a new tab (isolated browser page). Returns the tab ID; subsequent tool calls operate on the most recently opened or browser_tab_switch'd tab.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "url": { "type": "string", "description": "Optional URL to navigate the new tab to" }
                    }
                }
            },
            {
                "name": "browser_tab_list",
                "description": "List all open tabs with their ID, URL, title, and which one is active.",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "browser_tab_switch",
                "description": "Switch the active tab. All subsequent tool calls (snapshot, click, etc.) target this tab.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "tab_id": { "type": "string" }
                    },
                    "required": ["tab_id"]
                }
            },
            {
                "name": "browser_tab_close",
                "description": "Close a tab by ID (default: the active tab). If you close the active tab, the next remaining tab becomes active.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "tab_id": { "type": "string" }
                    }
                }
            },
            {
                "name": "browser_search",
                "description": "Find substring matches in the visible page text. Returns each match with its surrounding context. Use this to confirm content exists before scraping or to locate a section.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "case_sensitive": { "type": "boolean" },
                        "limit": { "type": "number", "description": format!("Max matches to return (default {max_search_results})") },
                        "context_chars": { "type": "number", "description": format!("Chars on each side of the match (default {search_context_chars})") }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "browser_storage_state",
                "description": "Export the full authentication / session state (cookies + localStorage + sessionStorage) as a JSON object. Save this to skip a login on a subsequent run via browser_set_storage_state.",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "browser_set_storage_state",
                "description": "Restore session state previously returned by browser_storage_state. Pass the JSON object. Use to bring an authenticated session back without re-logging in.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "state": {
                            "type": "object",
                            "description": "{cookies: [...], origins: [{origin, localStorage: [...], sessionStorage: [...]}]}"
                        }
                    },
                    "required": ["state"]
                }
            }
    ]).as_array().cloned().expect("MCP tool list must be an array");

    #[cfg(feature = "render")]
    {
        tools.extend([
            json!({
                "name": "browser_screenshot",
                "description": "Capture the current rendered viewport as a PNG image. Width and height default to the page's current CSS viewport.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "width": { "type": "number", "exclusiveMinimum": 0, "maximum": 32768, "description": "Optional CSS-pixel capture width" },
                        "height": { "type": "number", "exclusiveMinimum": 0, "maximum": 32768, "description": "Optional CSS-pixel capture height" }
                    },
                    "additionalProperties": false
                }
            }),
            json!({
                "name": "browser_pdf",
                "description": "Export the current rendered document as a paginated raster PDF using print media and bounded PDF defaults.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "landscape": { "type": "boolean" },
                        "print_background": { "type": "boolean" },
                        "scale": { "type": "number", "minimum": 0.1, "maximum": 2.0 },
                        "paper_width": { "type": "number", "exclusiveMinimum": 0, "maximum": 200, "description": "Paper width in inches" },
                        "paper_height": { "type": "number", "exclusiveMinimum": 0, "maximum": 200, "description": "Paper height in inches" },
                        "margin_top": { "type": "number", "minimum": 0, "description": "Top margin in inches" },
                        "margin_bottom": { "type": "number", "minimum": 0, "description": "Bottom margin in inches" },
                        "margin_left": { "type": "number", "minimum": 0, "description": "Left margin in inches" },
                        "margin_right": { "type": "number", "minimum": 0, "description": "Right margin in inches" }
                    },
                    "additionalProperties": false
                }
            }),
        ]);
    }

    RpcResponse::ok(id, json!({ "tools": tools }))
}

async fn handle_tool_call(id: Value, params: &Value, state: &mut BrowserState) -> RpcResponse {
    let name = match params.get("name").and_then(Value::as_str) {
        Some(n) => n,
        None => return RpcResponse::err(id, -32602, "Missing tool name"),
    };
    let args = params.get("arguments").unwrap_or(&Value::Null);

    #[cfg(feature = "render")]
    {
        let media_result = match name {
            "browser_screenshot" => Some(tool_screenshot(args, state).await),
            "browser_pdf" => Some(tool_pdf(args, state).await),
            _ => None,
        };
        if let Some(result) = media_result {
            return match result {
                Ok(content) => RpcResponse::ok(id, json!({ "content": [content] })),
                Err(error) => RpcResponse::ok(id, json!({
                    "content": [{ "type": "text", "text": format!("Error: {error}") }],
                    "isError": true
                })),
            };
        }
    }

    let result = match name {
        "browser_navigate" => tool_navigate(args, state).await,
        "browser_snapshot" => tool_snapshot(args, state),
        "browser_click" => tool_click(args, state).await,
        "browser_fill" => tool_fill(args, state).await,
        "browser_type" => tool_type(args, state).await,
        "browser_press_key" => tool_press_key(args, state).await,
        "browser_select_option" => tool_select_option(args, state),
        "browser_evaluate" => tool_evaluate(args, state).await,
        "browser_wait_for" => tool_wait_for(args, state).await,
        "browser_network_requests" => tool_network_requests(state),
        "browser_console_messages" => tool_console_messages(state),
        "browser_close" => tool_close(state),
        // Tier 1 agent-UX additions
        "browser_markdown" => tool_markdown(args, state),
        "browser_links" => tool_links(args, state),
        "browser_interactive_elements" => tool_interactive_elements(args, state),
        "browser_back" => tool_back(state).await,
        "browser_forward" => tool_forward(state).await,
        "browser_reload" => tool_reload(state).await,
        "browser_get_cookies" => tool_get_cookies(args, state),
        "browser_set_cookie" => tool_set_cookie(args, state),
        "browser_clear_cookies" => tool_clear_cookies(state),
        "browser_wait_for_text" => tool_wait_for_text(args, state).await,
        // Tier 2 agent-UX additions
        "browser_detect_forms" => tool_detect_forms(state),
        "browser_fill_form" => tool_fill_form(args, state),
        "browser_scroll" => tool_scroll(args, state),
        "browser_get_attribute" => tool_get_attribute(args, state),
        "browser_count" => tool_count(args, state),
        "browser_extract" => tool_extract(args, state),
        "browser_tab_new" => tool_tab_new(args, state).await,
        "browser_tab_list" => tool_tab_list(state),
        "browser_tab_switch" => tool_tab_switch(args, state),
        "browser_tab_close" => tool_tab_close(args, state),
        "browser_search" => tool_search(args, state),
        "browser_storage_state" => tool_storage_state(state),
        "browser_set_storage_state" => tool_set_storage_state(args, state),
        _ => Err(format!("Unknown tool: {name}")),
    };

    match result {
        Ok(content) => RpcResponse::ok(id, json!({
            "content": [{ "type": "text", "text": content }]
        })),
        Err(e) => RpcResponse::ok(id, json!({
            "content": [{ "type": "text", "text": format!("Error: {e}") }],
            "isError": true
        })),
    }
}

#[cfg(feature = "render")]
fn validate_tool_options(args: &Value, allowed: &[&str]) -> Result<(), String> {
    if args.is_null() {
        return Ok(());
    }
    let object = args.as_object().ok_or("tool arguments must be an object")?;
    if let Some(name) = object.keys().find(|name| !allowed.contains(&name.as_str())) {
        return Err(format!("unsupported option '{name}'"));
    }
    Ok(())
}

#[cfg(feature = "render")]
fn optional_number(args: &Value, name: &str) -> Result<Option<f32>, String> {
    let Some(value) = args.get(name) else {
        return Ok(None);
    };
    let value = value.as_f64().ok_or_else(|| format!("'{name}' must be a number"))?;
    if !value.is_finite() || value < f64::from(f32::MIN) || value > f64::from(f32::MAX) {
        return Err(format!("'{name}' must be a finite number"));
    }
    Ok(Some(value as f32))
}

#[cfg(feature = "render")]
fn optional_bool(args: &Value, name: &str) -> Result<Option<bool>, String> {
    let Some(value) = args.get(name) else {
        return Ok(None);
    };
    value.as_bool().map(Some).ok_or_else(|| format!("'{name}' must be a boolean"))
}

#[cfg(feature = "render")]
fn validate_screenshot_viewport(viewport: (f32, f32)) -> Result<(), String> {
    const MAX_DIMENSION: f32 = 32_768.0;
    const MAX_PIXELS: f64 = (16 * 1024 * 1024) as f64;
    let (width, height) = viewport;
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0
        || width > MAX_DIMENSION || height > MAX_DIMENSION
    {
        return Err("screenshot dimensions must be finite, positive, and at most 32768 CSS pixels".into());
    }
    if f64::from(width.ceil()) * f64::from(height.ceil()) > MAX_PIXELS {
        return Err("screenshot dimensions exceed the 16-megapixel capture limit".into());
    }
    Ok(())
}

#[cfg(feature = "render")]
async fn tool_screenshot(args: &Value, state: &mut BrowserState) -> Result<Value, String> {
    validate_tool_options(args, &["width", "height"])?;
    let current = state.page_mut().viewport;
    let viewport = (
        optional_number(args, "width")?.unwrap_or(current.0),
        optional_number(args, "height")?.unwrap_or(current.1),
    );
    validate_screenshot_viewport(viewport)?;

    let page = state.page_mut();
    let _ = page.prepare_screenshot_resources(1_000).await;
    let png = page.screenshot(viewport).ok_or("the current page has no renderable viewport")?;
    Ok(json!({
        "type": "image",
        "data": BASE64.encode(png),
        "mimeType": "image/png"
    }))
}

#[cfg(feature = "render")]
async fn tool_pdf(args: &Value, state: &mut BrowserState) -> Result<Value, String> {
    validate_tool_options(args, &[
        "landscape", "print_background", "scale", "paper_width", "paper_height",
        "margin_top", "margin_bottom", "margin_left", "margin_right",
    ])?;
    let mut options = telemaco_browser::RasterPdfOptions::default();
    if let Some(value) = optional_bool(args, "landscape")? { options.landscape = value; }
    if let Some(value) = optional_bool(args, "print_background")? { options.print_background = value; }
    if let Some(value) = optional_number(args, "scale")? { options.scale = value; }
    if let Some(value) = optional_number(args, "paper_width")? { options.paper_width_in = value; }
    if let Some(value) = optional_number(args, "paper_height")? { options.paper_height_in = value; }
    if let Some(value) = optional_number(args, "margin_top")? { options.margin_top_in = value; }
    if let Some(value) = optional_number(args, "margin_bottom")? { options.margin_bottom_in = value; }
    if let Some(value) = optional_number(args, "margin_left")? { options.margin_left_in = value; }
    if let Some(value) = optional_number(args, "margin_right")? { options.margin_right_in = value; }

    let page = state.page_mut();
    let _ = page.prepare_screenshot_resources(1_000).await;
    let pdf = page.raster_pdf(options).map_err(|error| error.to_string())?;
    Ok(json!({
        "type": "resource",
        "resource": {
            "uri": "telemaco://capture/current-page.pdf",
            "mimeType": "application/pdf",
            "blob": BASE64.encode(pdf)
        }
    }))
}

/// Resolve a tool call's element target from either `ref` (preferred) or
/// `selector` (fallback). Agents that called `browser_snapshot` /
/// `browser_interactive_elements` get a ref table they can refer to;
/// scripted clients can still pass raw CSS selectors.
fn resolve_target(args: &Value, state: &BrowserState) -> Result<String, String> {
    if let Some(r) = args.get("ref").and_then(Value::as_str) {
        return state.ref_to_selector(r);
    }
    if let Some(sel) = args.get("selector").and_then(Value::as_str) {
        return Ok(sel.to_string());
    }
    Err("Missing 'ref' or 'selector' parameter".to_string())
}

/// Marker appended when a list result was capped.
///
/// A capped result must never look like a complete one. This is the same
/// failure the empty-`browser_markdown` case exposed: output that carries no
/// statement about its own completeness sends the agent chasing a phantom.
fn omitted_note(shown: usize, total: usize) -> String {
    if total <= shown {
        return String::new();
    }
    format!(
        "\n...({} of {total} omitted; raise the cap via config or the tool's limit argument)",
        total - shown
    )
}

/// Clamp text to `max_chars` and tack on a `...(truncated, N more chars)`
/// marker so the agent can ask for more if needed. The ceiling is configurable
/// (see [`crate::config`]) to keep a single tool call from consuming a window
/// of context.
fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let head: String = text.chars().take(max_chars).collect();
    let remaining = text.chars().count() - max_chars;
    format!("{head}\n...(truncated, {remaining} more chars)")
}

async fn tool_navigate(args: &Value, state: &mut BrowserState) -> Result<String, String> {
    let url = args.get("url").and_then(Value::as_str)
        .ok_or("Missing url parameter")?;
    let wait_until = args.get("waitUntil").and_then(Value::as_str).unwrap_or("load");

    let condition = telemaco_browser::lifecycle::WaitUntil::from_str(wait_until);
    let ua = state.user_agent.clone();
    let page = state.page_mut();
    if let Some(ref ua) = ua {
        page.http_client.set_user_agent(ua).await;
    }

    page.navigate_with_wait(url, condition).await
        .map_err(|e| e.to_string())?;

    let summary = format!("Navigated to {} — \"{}\"", page.url_string(), page.title);
    // DOM changed — invalidate the ref table. Next snapshot will rebuild.
    state.interactive_refs.clear();
    // DOM changed, so the markdown cache is stale even if the URL is the
    // same (refresh of the same page). Next browser_markdown re-converts.
    state.markdown_cache = None;
    Ok(summary)
}

fn tool_snapshot(args: &Value, state: &mut BrowserState) -> Result<String, String> {
    let max_chars = ExtractionLimits::override_with(
        state.limits.max_chars,
        args.get("max_chars").and_then(Value::as_u64),
    );
    rebuild_interactive_refs(state)?;
    let page = state.page_mut();
    let url = page.url_string();
    let title = page.title.clone();

    let body_text = page.with_dom(|dom| {
        if let Ok(Some(body)) = dom.query_selector("body") {
            extract_text(dom, body)
        } else {
            String::new()
        }
    }).unwrap_or_default();

    let refs_summary = if state.interactive_refs.is_empty() {
        String::new()
    } else {
        format!(
            "\n\n{} interactive element(s) registered. Call browser_interactive_elements to list, or pass `ref` to browser_click/browser_fill/browser_type.",
            state.interactive_refs.len(),
        )
    };

    let body = truncate(body_text.trim(), max_chars);
    Ok(format!("URL: {url}\nTitle: {title}\n\n{body}{refs_summary}"))
}

async fn tool_click(args: &Value, state: &mut BrowserState) -> Result<String, String> {
    let selector = resolve_target(args, state)?;

    let js = format!(
        r#"(function(){{
            var el = document.querySelector({sel});
            if (!el) return "error:element not found";
            el.click();
            return "ok";
        }})()"#,
        sel = serde_json::to_string(&selector).unwrap()
    );

    let result = state.page_mut().evaluate(&js);
    if result.as_str() == Some("error:element not found") {
        Err(format!("Element not found: {selector}"))
    } else {
        // A click can navigate or rewrite the DOM; the old ref table may
        // no longer match. Conservative: invalidate. Next snapshot rebuilds.
        state.interactive_refs.clear();
        state.settle_synthetic_navigation().await?;
        Ok(format!("Clicked '{selector}'"))
    }
}

async fn tool_fill(args: &Value, state: &mut BrowserState) -> Result<String, String> {
    let selector = resolve_target(args, state)?;
    let value = args.get("value").and_then(Value::as_str)
        .ok_or("Missing value parameter")?;

    let js = format!(
        r#"(function(){{
            var el = document.querySelector({sel});
            if (!el) return "error:element not found";
            globalThis.__telemaco_setFieldValue(el, "value", {val});
            el.dispatchEvent(globalThis.__telemaco_markTrusted(new Event("input", {{bubbles:true}})));
            el.dispatchEvent(globalThis.__telemaco_markTrusted(new Event("change", {{bubbles:true}})));
            return "ok";
        }})()"#,
        sel = serde_json::to_string(&selector).unwrap(),
        val = serde_json::to_string(value).unwrap()
    );

    let result = state.page_mut().evaluate(&js);
    if result.as_str() == Some("error:element not found") {
        Err(format!("Element not found: {selector}"))
    } else {
        state.settle_synthetic_navigation().await?;
        Ok(format!("Filled '{selector}' with value"))
    }
}

async fn tool_type(args: &Value, state: &mut BrowserState) -> Result<String, String> {
    let selector = resolve_target(args, state)?;
    let text = args.get("text").and_then(Value::as_str)
        .ok_or("Missing text parameter")?;

    let js = format!(
        r#"(function(){{
            var el = document.querySelector({sel});
            if (!el) return "error:element not found";
            globalThis.__telemaco_setFieldValue(el, "value", (el.value || "") + {txt});
            el.dispatchEvent(globalThis.__telemaco_markTrusted(new Event("input", {{bubbles:true}})));
            return "ok";
        }})()"#,
        sel = serde_json::to_string(&selector).unwrap(),
        txt = serde_json::to_string(text).unwrap()
    );

    let result = state.page_mut().evaluate(&js);
    if result.as_str() == Some("error:element not found") {
        Err(format!("Element not found: {selector}"))
    } else {
        state.settle_synthetic_navigation().await?;
        Ok(format!("Typed into '{selector}'"))
    }
}

async fn tool_press_key(args: &Value, state: &mut BrowserState) -> Result<String, String> {
    let key = args.get("key").and_then(Value::as_str)
        .ok_or("Missing key parameter")?;
    let selector = args.get("selector").and_then(Value::as_str);

    let target = match selector {
        Some(sel) => format!("document.querySelector({})", serde_json::to_string(sel).unwrap()),
        None => "document".to_string(),
    };

    let js = format!(
        r#"(function(){{
            var t = {target};
            if (!t) return "error:element not found";
            t.dispatchEvent(new KeyboardEvent("keydown", {{key:{key},bubbles:true}}));
            t.dispatchEvent(new KeyboardEvent("keyup", {{key:{key},bubbles:true}}));
            return "ok";
        }})()"#,
        target = target,
        key = serde_json::to_string(key).unwrap()
    );

    state.page_mut().evaluate(&js);
    state.settle_synthetic_navigation().await?;
    Ok(format!("Pressed key '{key}'"))
}

fn tool_select_option(args: &Value, state: &mut BrowserState) -> Result<String, String> {
    let selector = args.get("selector").and_then(Value::as_str)
        .ok_or("Missing selector parameter")?;
    let value = args.get("value").and_then(Value::as_str)
        .ok_or("Missing value parameter")?;

    let js = format!(
        r#"(function(){{
            var el = document.querySelector({sel});
            if (!el) return "error:element not found";
            var opts = Array.from(el.options);
            var opt = opts.find(function(o){{ return o.value === {val} || o.text === {val}; }});
            if (!opt) return "error:option not found";
            el.value = opt.value;
            el.dispatchEvent(new Event("change", {{bubbles:true}}));
            return "ok";
        }})()"#,
        sel = serde_json::to_string(selector).unwrap(),
        val = serde_json::to_string(value).unwrap()
    );

    let result = state.page_mut().evaluate(&js);
    match result.as_str() {
        Some("error:element not found") => Err(format!("Element not found: {selector}")),
        Some("error:option not found") => Err(format!("Option not found: {value}")),
        _ => Ok(format!("Selected '{value}' in '{selector}'")),
    }
}

async fn tool_evaluate(args: &Value, state: &mut BrowserState) -> Result<String, String> {
    let expression = args.get("expression").and_then(Value::as_str)
        .ok_or("Missing expression parameter")?;

    let result = state.page_mut().evaluate(expression);
    state.settle_synthetic_navigation().await?;
    Ok(match &result {
        Value::String(s) => s.clone(),
        Value::Null => "null".to_string(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    })
}

async fn tool_wait_for(args: &Value, state: &mut BrowserState) -> Result<String, String> {
    let selector = args.get("selector").and_then(Value::as_str)
        .ok_or("Missing selector parameter")?;
    let timeout_secs = args.get("timeout").and_then(Value::as_f64).unwrap_or(30.0) as u64;

    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(timeout_secs);
    // Exponential backoff: 5 -> 10 -> 20 -> ... -> 200 ms. The old fixed
    // 200ms tick added up to a full poll cycle of latency every time;
    // a selector that appears in 30ms now returns in ~35ms instead of
    // the next 200ms tick.
    let mut tick_ms: u64 = 5;
    loop {
        let found = state.page_mut().with_dom(|dom| {
            dom.query_selector(selector).ok().flatten().is_some()
        }).unwrap_or(false);

        if found {
            return Ok(format!("Found '{selector}'"));
        }

        if tokio::time::Instant::now() >= deadline {
            return Err(format!("Timeout waiting for '{selector}'"));
        }

        let tick = tokio::time::Duration::from_millis(tick_ms);
        match tokio::time::timeout(tick, state.advance_active_page_tasks()).await {
            Ok(result) => {
                result?;
            }
            Err(_) => {}
        }
        if tick_ms < 200 { tick_ms = (tick_ms * 2).min(200); }
    }
}

fn tool_network_requests(state: &mut BrowserState) -> Result<String, String> {
    let limit = ExtractionLimits::cap(state.limits.max_network_requests);
    let page = state.page_mut();
    let events = &page.network_events;

    if events.is_empty() {
        return Ok("No network requests recorded.".to_string());
    }

    let total = events.len();
    let lines: Vec<String> = events.iter().take(limit).map(|e| {
        format!("[{}] {} {} ({}B)", e.status, e.method, e.url, e.body_size)
    }).collect();
    let shown = lines.len();

    Ok(format!("{}{}", lines.join("\n"), omitted_note(shown, total)))
}

fn tool_console_messages(state: &BrowserState) -> Result<String, String> {
    if state.console_messages.is_empty() {
        return Ok("No console messages.".to_string());
    }
    let limit = ExtractionLimits::cap(state.limits.max_console_messages);
    let total = state.console_messages.len();
    let shown: Vec<&str> = state.console_messages.iter().take(limit).map(String::as_str).collect();
    Ok(format!("{}{}", shown.join("\n"), omitted_note(shown.len(), total)))
}

fn tool_close(state: &mut BrowserState) -> Result<String, String> {
    // Drop the one live isolate (if any) via suspend_js before clearing, so the
    // map drop disposes no isolate and the LIFO rule holds regardless of the
    // BTreeMap's ascending drop order (#258).
    for page in state.tabs.values_mut() {
        page.suspend_js();
    }
    state.tabs.clear();
    state.active_tab = None;
    state.console_messages.clear();
    state.interactive_refs.clear();
    Ok("All browser tabs closed.".to_string())
}

// ===== Tier 1 agent-UX additions =====

/// Convert the rendered page to Markdown by running the JS-side converter
/// already used by `telemaco fetch --dump markdown`. More token-dense than
/// browser_snapshot for content-heavy pages (article bodies, docs sites).
///
/// The full document is converted once and cached per URL; the `page`
/// parameter (1-based) returns successive block-boundary chunks of the
/// cached document without touching V8 again. `refresh: true` forces
/// re-conversion of the current page.
fn tool_markdown(args: &Value, state: &mut BrowserState) -> Result<String, String> {
    // The page size comes from the resolved config, not a constant: this is
    // the layer the CLI flag, the environment, and the config file all feed.
    // `override_with` also maps the 0 sentinel to usize::MAX, which packs the
    // whole document into a single page and, as a side effect, keeps a zero
    // from ever reaching split_long_line, where it would slice at a byte that
    // is not a character boundary.
    let max_chars = ExtractionLimits::override_with(
        state.limits.max_chars,
        args.get("max_chars").and_then(Value::as_u64),
    );
    let page_no = args.get("page").and_then(Value::as_u64).map(|n| n as usize)
        .unwrap_or(1).max(1);
    let refresh = args.get("refresh").and_then(Value::as_bool).unwrap_or(false);

    let url = state.page_mut().url_string();
    let md: String = match &state.markdown_cache {
        Some((cached_url, cached_md)) if !refresh && cached_url == &url => cached_md.clone(),
        _ => {
            let page = state.page_mut();
            let result = page.evaluate(telemaco_browser::HTML_TO_MARKDOWN_JS);
            let md = result.as_str().unwrap_or_default().to_string();
            state.markdown_cache = Some((url, md.clone()));
            md
        }
    };

    let (text, total) = blocks::paginate(&md, max_chars, page_no);
    if total == 0 {
        return Ok("(empty page: nothing to convert)".to_string());
    }
    if text.is_empty() {
        return Ok(format!(
            "Page {page_no} is beyond the end of this document ({total} page(s) total). Call browser_markdown with page=1..{total}."
        ));
    }
    if page_no < total {
        Ok(format!(
            "{text}\n\n<!-- markdown page {page_no} of {total} (call browser_markdown with page={} for more) -->",
            page_no + 1
        ))
    } else {
        Ok(format!("{text}\n\n<!-- markdown page {page_no} of {total} (end of document) -->"))
    }
}

/// Enumerate every `<a href>` on the page. One JSON object per line so
/// the agent can grep / split without round-tripping to a JSON parser.
fn tool_links(args: &Value, state: &mut BrowserState) -> Result<String, String> {
    let limit = ExtractionLimits::override_with(
        state.limits.max_links,
        args.get("limit").and_then(Value::as_u64),
    );
    let internal_only = args.get("internal_only").and_then(Value::as_bool).unwrap_or(false);
    let page = state.page_mut();
    let base_origin = url::Url::parse(&page.url_string())
        .ok()
        .map(|u| u.origin())
        .unwrap_or_else(|| url::Url::parse("about:blank").unwrap().origin());

    let js = r#"(function(){
        var out = [];
        var seen = new Set();
        var as = document.querySelectorAll('a[href]');
        for (var i = 0; i < as.length; i++) {
            var a = as[i];
            var href = a.href || '';
            if (!href || href === '#' || href.startsWith('javascript:')) continue;
            if (seen.has(href)) continue;
            seen.add(href);
            var t = (a.innerText || a.textContent || '').trim().replace(/\s+/g, ' ').slice(0, 200);
            out.push({text: t, href: href});
        }
        return out;
    })()"#;
    let val = page.evaluate(js);
    let arr = val.as_array().cloned().unwrap_or_default();
    let lines: Vec<String> = arr.into_iter()
        .filter(|item| {
            if !internal_only { return true; }
            item.get("href").and_then(|v| v.as_str())
                .and_then(|h| url::Url::parse(h).ok())
                .map(|u| u.origin() == base_origin)
                .unwrap_or(false)
        })
        .take(limit)
        .map(|item| item.to_string())
        .collect();
    if lines.is_empty() {
        Ok("No links found.".to_string())
    } else {
        Ok(lines.join("\n"))
    }
}

/// List every interactable element with a stable ref ID, the kind of
/// element, and a one-line description. Agents pass `ref` to click/fill/
/// type instead of crafting selectors. Also assigns `data-telemaco-ref`
/// to each element so the ref survives until the next navigation.
fn tool_interactive_elements(args: &Value, state: &mut BrowserState) -> Result<String, String> {
    let limit = ExtractionLimits::override_with(
        state.limits.max_interactive,
        args.get("limit").and_then(Value::as_u64),
    );
    // The limit is interpolated into the JS below, so an unlimited cap must not
    // emit usize::MAX as a literal: JS would widen it to a float anyway, and the
    // intent reads better spelled out.
    let limit = if limit == usize::MAX {
        "Number.MAX_SAFE_INTEGER".to_string()
    } else {
        limit.to_string()
    };
    rebuild_interactive_refs(state)?;
    if state.interactive_refs.is_empty() {
        return Ok("No interactive elements on this page.".to_string());
    }
    let page = state.page_mut();
    let js = format!(r#"(function(){{
        var els = document.querySelectorAll('[data-telemaco-ref]');
        var out = [];
        for (var i = 0; i < els.length && out.length < {limit}; i++) {{
            var e = els[i];
            var label = (e.innerText || e.textContent || e.getAttribute('aria-label') || e.getAttribute('placeholder') || e.getAttribute('value') || e.getAttribute('name') || '').trim().replace(/\s+/g, ' ').slice(0, 80);
            var role = e.getAttribute('role') || '';
            var typeAttr = e.getAttribute('type') || '';
            out.push({{
                ref: e.getAttribute('data-telemaco-ref'),
                tag: e.tagName.toLowerCase(),
                type: typeAttr,
                role: role,
                name: e.getAttribute('name') || '',
                label: label,
            }});
        }}
        return out;
    }})()"#);
    let val = page.evaluate(&js);
    let arr = val.as_array().cloned().unwrap_or_default();
    let lines: Vec<String> = arr.into_iter().map(|item| {
        let r = item.get("ref").and_then(|v| v.as_str()).unwrap_or("?");
        let tag = item.get("tag").and_then(|v| v.as_str()).unwrap_or("?");
        let ty = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let label = item.get("label").and_then(|v| v.as_str()).unwrap_or("");
        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("");
        let kind = if !ty.is_empty() { format!("{tag}[{ty}]") } else if !role.is_empty() { format!("{tag}[role={role}]") } else { tag.to_string() };
        let detail = if !name.is_empty() { format!(" name={name:?}") } else { String::new() };
        format!("ref={r:<5} {kind:<22} {label:?}{detail}")
    }).collect();
    Ok(lines.join("\n"))
}

/// Rebuild the ref table: walk the DOM, find every interactable, assign
/// a stable `data-telemaco-ref="eN"` attribute, remember the nid for later
/// validation. Called on every snapshot / interactive-elements call so the
/// agent always sees fresh refs.
fn rebuild_interactive_refs(state: &mut BrowserState) -> Result<(), String> {
    state.interactive_refs.clear();
    let page = state.page_mut();
    // Tag every interactable with data-telemaco-ref="eN" in DOM order.
    let tag_js = r#"(function(){
        var sel = 'a[href], button, input:not([type=hidden]), select, textarea, [role=button], [role=link], [role=checkbox], [role=tab], [role=menuitem], [role=option], [onclick], [tabindex]:not([tabindex="-1"])';
        var els = document.querySelectorAll(sel);
        var refs = [];
        for (var i = 0; i < els.length; i++) {
            var ref = 'e' + (i + 1);
            els[i].setAttribute('data-telemaco-ref', ref);
            refs.push(ref);
        }
        return refs;
    })()"#;
    let val = page.evaluate(tag_js);
    let refs: Vec<String> = val.as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    // Map ref -> nid via a second pass so ref_to_selector can sanity-check.
    for r in refs {
        let selector = format!("[data-telemaco-ref=\"{r}\"]");
        let page = state.page_mut();
        let nid = page.with_dom(|dom| dom.query_selector(&selector).ok().flatten());
        if let Some(Some(node_id)) = nid {
            state.interactive_refs.insert(r, node_id);
        }
    }
    Ok(())
}

async fn tool_back(state: &mut BrowserState) -> Result<String, String> {
    let history_url = state.page_mut().with_dom(|_| ()).map(|_| ());
    let _ = history_url;
    // We track simple page history on the Page itself; navigate to the
    // entry before the cursor.
    let page = state.page_mut();
    if page.history.len() < 2 || page.history_index == 0 {
        return Err("No previous page in history.".to_string());
    }
    let prev_idx = page.history_index - 1;
    let url = page.history[prev_idx].clone();
    page.set_history_index(prev_idx);
    let condition = telemaco_browser::lifecycle::WaitUntil::DomContentLoaded;
    let stash = (page.history.clone(), page.history_index);
    page.navigate_with_wait(&url, condition).await.map_err(|e| e.to_string())?;
    let page = state.page_mut();
    page.history = stash.0;
    page.history_index = stash.1;
    state.interactive_refs.clear();
    Ok(format!("Back to {url}"))
}

async fn tool_forward(state: &mut BrowserState) -> Result<String, String> {
    let page = state.page_mut();
    if page.history_index + 1 >= page.history.len() {
        return Err("No forward page in history.".to_string());
    }
    let next_idx = page.history_index + 1;
    let url = page.history[next_idx].clone();
    page.set_history_index(next_idx);
    let condition = telemaco_browser::lifecycle::WaitUntil::DomContentLoaded;
    let stash = (page.history.clone(), page.history_index);
    page.navigate_with_wait(&url, condition).await.map_err(|e| e.to_string())?;
    let page = state.page_mut();
    page.history = stash.0;
    page.history_index = stash.1;
    state.interactive_refs.clear();
    Ok(format!("Forward to {url}"))
}

async fn tool_reload(state: &mut BrowserState) -> Result<String, String> {
    let url = state.page_mut().url_string();
    if url == "about:blank" {
        return Err("Nothing to reload.".to_string());
    }
    let condition = telemaco_browser::lifecycle::WaitUntil::DomContentLoaded;
    state.page_mut().navigate_with_wait(&url, condition).await.map_err(|e| e.to_string())?;
    state.interactive_refs.clear();
    Ok(format!("Reloaded {url}"))
}

fn tool_get_cookies(args: &Value, state: &BrowserState) -> Result<String, String> {
    let domain_filter = args.get("domain").and_then(Value::as_str);
    let cookies = state.context.cookie_jar.get_all_cookies();
    let lines: Vec<String> = cookies.iter()
        .filter(|c| domain_filter.is_none_or(|d| c.domain == telemaco_net::canonical_domain(d)))
        .map(|c| serde_json::to_string(&json!({
            "name": c.name,
            "value": c.value,
            "domain": c.domain,
            "path": c.path,
            "secure": c.secure,
            "http_only": c.http_only,
        })).unwrap_or_default())
        .collect();
    if lines.is_empty() {
        Ok("No cookies.".to_string())
    } else {
        Ok(lines.join("\n"))
    }
}

fn tool_set_cookie(args: &Value, state: &BrowserState) -> Result<String, String> {
    let name = args.get("name").and_then(Value::as_str)
        .ok_or("Missing name parameter")?;
    let value = args.get("value").and_then(Value::as_str)
        .ok_or("Missing value parameter")?;
    let domain = args.get("domain").and_then(Value::as_str)
        .ok_or("Missing domain parameter")?;
    let path = args.get("path").and_then(Value::as_str).unwrap_or("/");
    let secure = args.get("secure").and_then(Value::as_bool).unwrap_or(false);
    let http_only = args.get("http_only").and_then(Value::as_bool).unwrap_or(false);
    let cookie = telemaco_net::CookieInfo {
        name: name.to_string(),
        value: value.to_string(),
        domain: domain.to_string(),
        path: path.to_string(),
        secure,
        http_only,
        same_site: String::new(),
        expires: None,
    };
    state.context.cookie_jar.set_cookies_from_cdp(vec![cookie]);
    Ok(format!("Set cookie {name} on {domain}{path}"))
}

fn tool_clear_cookies(state: &BrowserState) -> Result<String, String> {
    state.context.cookie_jar.clear();
    Ok("Cleared all cookies.".to_string())
}

async fn tool_wait_for_text(args: &Value, state: &mut BrowserState) -> Result<String, String> {
    let needle = args.get("text").and_then(Value::as_str)
        .ok_or("Missing text parameter")?;
    let timeout_secs = args.get("timeout").and_then(Value::as_f64).unwrap_or(30.0) as u64;
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(timeout_secs);
    let escaped = serde_json::to_string(needle).unwrap_or_else(|_| "\"\"".to_string());
    let js = format!(r#"(function(){{
        var t = (document.body && (document.body.innerText || document.body.textContent)) || '';
        return t.indexOf({needle}) >= 0;
    }})()"#, needle = escaped);
    // Exponential backoff like browser_wait_for (see comment there).
    let mut tick_ms: u64 = 5;
    loop {
        let found = state.page_mut().evaluate(&js).as_bool().unwrap_or(false);
        if found {
            return Ok(format!("Found text {needle:?}"));
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(format!("Timeout waiting for text {needle:?}"));
        }
        let tick = tokio::time::Duration::from_millis(tick_ms);
        match tokio::time::timeout(tick, state.advance_active_page_tasks()).await {
            Ok(result) => {
                result?;
            }
            Err(_) => {}
        }
        if tick_ms < 200 { tick_ms = (tick_ms * 2).min(200); }
    }
}

// ===== Tier 2 agent-UX additions =====

/// Describe every <form> on the page. For each form, list its action,
/// method, and every field (input/select/textarea) with its name, type,
/// current value, and any visible label text. Agents call this to
/// understand a form's shape before filling it.
fn tool_detect_forms(state: &mut BrowserState) -> Result<String, String> {
    let limit = ExtractionLimits::cap(state.limits.max_forms);
    let page = state.page_mut();
    let js = r#"(function(){
        var forms = document.querySelectorAll('form');
        var out = [];
        for (var i = 0; i < forms.length; i++) {
            var f = forms[i];
            var fields = [];
            var inputs = f.querySelectorAll('input, select, textarea, button');
            for (var j = 0; j < inputs.length; j++) {
                var el = inputs[j];
                var tag = el.tagName.toLowerCase();
                var type = (el.getAttribute('type') || (tag === 'input' ? 'text' : tag)).toLowerCase();
                if (tag === 'input' && type === 'hidden') continue;
                var name = el.getAttribute('name') || '';
                var label = '';
                if (el.id) {
                    var lab = document.querySelector('label[for="' + el.id + '"]');
                    if (lab) label = (lab.innerText || lab.textContent || '').trim();
                }
                if (!label) label = el.getAttribute('aria-label') || el.getAttribute('placeholder') || '';
                var opts = null;
                if (tag === 'select') {
                    opts = [];
                    var os = el.querySelectorAll('option');
                    for (var k = 0; k < os.length; k++) {
                        opts.push({ value: os[k].value, text: (os[k].textContent || '').trim() });
                    }
                }
                fields.push({
                    tag: tag,
                    type: type,
                    name: name,
                    value: el.value || '',
                    checked: el.checked || false,
                    required: el.required || false,
                    label: label.trim().slice(0, 100),
                    ref: el.getAttribute('data-telemaco-ref') || null,
                    options: opts,
                });
            }
            out.push({
                index: i,
                id: f.id || '',
                name: f.getAttribute('name') || '',
                action: f.action || '',
                method: (f.method || 'get').toLowerCase(),
                fields: fields,
            });
        }
        return out;
    })()"#;
    let val = page.evaluate(js);
    if val.is_null() {
        return Ok("No forms found.".to_string());
    }

    // Cap on the Rust side rather than inside the JS: the script above is a raw
    // string literal, and threading a limit through it would mean escaping every
    // brace in it for `format!`.
    let (val, total) = match val.as_array() {
        Some(forms) => {
            let total = forms.len();
            let kept: Vec<Value> = forms.iter().take(limit).cloned().collect();
            (Value::Array(kept), total)
        }
        None => (val, 0),
    };
    let shown = val.as_array().map(Vec::len).unwrap_or(0);

    let body = serde_json::to_string_pretty(&val).map_err(|e| e.to_string())?;
    Ok(format!("{body}{}", omitted_note(shown, total)))
}

/// Fill multiple fields in one call. Each entry: {ref|selector, value, type?}.
/// type='text' (default) sets value, 'check'/'uncheck' toggles checkbox,
/// 'select' picks an option by value or visible text. Optional
/// `submit_ref`/`submit_selector` clicks after filling.
fn tool_fill_form(args: &Value, state: &mut BrowserState) -> Result<String, String> {
    let fields = args.get("fields").and_then(Value::as_array)
        .ok_or("Missing fields array")?
        .clone();
    let mut filled = 0u32;
    let mut errors = Vec::new();
    for field in fields {
        let value = field.get("value").and_then(Value::as_str).unwrap_or("");
        let kind = field.get("type").and_then(Value::as_str).unwrap_or("text");
        let selector = match resolve_target(&field, state) {
            Ok(s) => s,
            Err(e) => { errors.push(e); continue; }
        };
        let js = match kind {
            "check" => format!(r#"(function(){{
                var el = document.querySelector({sel});
                if (!el) return "error:not found";
                globalThis.__telemaco_setFieldValue(el, 'checked', true);
                el.dispatchEvent(globalThis.__telemaco_markTrusted(new Event('input', {{bubbles:true}})));
                el.dispatchEvent(globalThis.__telemaco_markTrusted(new Event('change', {{bubbles:true}})));
                return "ok";
            }})()"#, sel = serde_json::to_string(&selector).unwrap()),
            "uncheck" => format!(r#"(function(){{
                var el = document.querySelector({sel});
                if (!el) return "error:not found";
                globalThis.__telemaco_setFieldValue(el, 'checked', false);
                el.dispatchEvent(globalThis.__telemaco_markTrusted(new Event('input', {{bubbles:true}})));
                el.dispatchEvent(globalThis.__telemaco_markTrusted(new Event('change', {{bubbles:true}})));
                return "ok";
            }})()"#, sel = serde_json::to_string(&selector).unwrap()),
            "select" => format!(r#"(function(){{
                var el = document.querySelector({sel});
                if (!el) return "error:not found";
                var want = {val};
                var matched = false;
                for (var i = 0; i < el.options.length; i++) {{
                    var o = el.options[i];
                    if (o.value === want || (o.textContent || '').trim() === want) {{
                        el.selectedIndex = i;
                        matched = true;
                        break;
                    }}
                }}
                if (!matched) return "error:no matching option";
                el.dispatchEvent(globalThis.__telemaco_markTrusted(new Event('input', {{bubbles:true}})));
                el.dispatchEvent(globalThis.__telemaco_markTrusted(new Event('change', {{bubbles:true}})));
                return "ok";
            }})()"#, sel = serde_json::to_string(&selector).unwrap(), val = serde_json::to_string(value).unwrap()),
            _ => format!(r#"(function(){{
                var el = document.querySelector({sel});
                if (!el) return "error:not found";
                globalThis.__telemaco_setFieldValue(el, 'value', {val});
                el.dispatchEvent(globalThis.__telemaco_markTrusted(new Event('input', {{bubbles:true}})));
                el.dispatchEvent(globalThis.__telemaco_markTrusted(new Event('change', {{bubbles:true}})));
                return "ok";
            }})()"#, sel = serde_json::to_string(&selector).unwrap(), val = serde_json::to_string(value).unwrap()),
        };
        let res = state.page_mut().evaluate(&js);
        match res.as_str() {
            Some("ok") => filled += 1,
            Some(e) => errors.push(format!("{selector}: {e}")),
            None => errors.push(format!("{selector}: unknown error")),
        }
    }

    // Optional submit click
    let submit_target = if args.get("submit_ref").is_some() || args.get("submit_selector").is_some() {
        let pseudo = json!({
            "ref": args.get("submit_ref"),
            "selector": args.get("submit_selector"),
        });
        resolve_target(&pseudo, state).ok()
    } else { None };
    if let Some(sel) = submit_target {
        let js = format!(r#"(function(){{
            var el = document.querySelector({sel});
            if (!el) return "error:not found";
            el.click();
            return "ok";
        }})()"#, sel = serde_json::to_string(&sel).unwrap());
        let _ = state.page_mut().evaluate(&js);
        state.interactive_refs.clear();
    }

    if errors.is_empty() {
        Ok(format!("Filled {filled} fields."))
    } else {
        Ok(format!("Filled {filled} fields. Errors: {}", errors.join("; ")))
    }
}

/// Scroll the page (or an element) by direction + amount, or scroll an
/// element into view. Used to trigger infinite-scroll loaders or to
/// reach off-viewport content. Returns the new scroll position.
fn tool_scroll(args: &Value, state: &mut BrowserState) -> Result<String, String> {
    let direction = args.get("direction").and_then(Value::as_str).unwrap_or("down");
    let amount = args.get("amount").and_then(Value::as_f64);

    // Element scroll-into-view path
    if args.get("ref").is_some() || args.get("selector").is_some() {
        let selector = resolve_target(args, state)?;
        let js = format!(r#"(function(){{
            var el = document.querySelector({sel});
            if (!el) return "error:not found";
            el.scrollIntoView({{behavior:'instant', block:'center'}});
            return JSON.stringify({{x: window.scrollX, y: window.scrollY}});
        }})()"#, sel = serde_json::to_string(&selector).unwrap());
        let res = state.page_mut().evaluate(&js);
        if res.as_str() == Some("error:not found") {
            return Err(format!("Element not found: {selector}"));
        }
        return Ok(format!("Scrolled element into view. {}", res.as_str().unwrap_or("")));
    }

    // Page-level scroll. Also dispatch a 'scroll' event so infinite-
    // scroll handlers fire (we don't have a real layout engine, so the
    // window.scrollY value won't change but the event is what matters).
    let amt = amount.unwrap_or(720.0);
    let js = format!(r#"(function(){{
        var dir = {dir};
        var amt = {amt};
        switch (dir) {{
            case 'top': window.scrollTo(0, 0); break;
            case 'bottom': window.scrollTo(0, document.body.scrollHeight); break;
            case 'up': window.scrollBy(0, -amt); break;
            case 'down': window.scrollBy(0, amt); break;
            case 'left': window.scrollBy(-amt, 0); break;
            case 'right': window.scrollBy(amt, 0); break;
        }}
        try {{ window.dispatchEvent(new Event('scroll', {{bubbles:true}})); }} catch(e) {{}}
        try {{ document.dispatchEvent(new Event('scroll', {{bubbles:true}})); }} catch(e) {{}}
        return JSON.stringify({{x: window.scrollX, y: window.scrollY, max_y: document.body.scrollHeight, viewport_h: window.innerHeight}});
    }})()"#,
        dir = serde_json::to_string(direction).unwrap(),
        amt = amt,
    );
    let res = state.page_mut().evaluate(&js);
    // A scroll can reveal new DOM (infinite scroll); invalidate refs.
    state.interactive_refs.clear();
    Ok(format!("Scrolled {direction}. {}", res.as_str().unwrap_or("")))
}

fn tool_get_attribute(args: &Value, state: &mut BrowserState) -> Result<String, String> {
    let selector = resolve_target(args, state)?;
    let attr = args.get("attribute").and_then(Value::as_str)
        .ok_or("Missing attribute parameter")?;
    let js = format!(r#"(function(){{
        var el = document.querySelector({sel});
        if (!el) return null;
        var v = el.getAttribute({a});
        if (v === null && {a} === 'value') v = el.value || '';
        return v == null ? '' : v;
    }})()"#,
        sel = serde_json::to_string(&selector).unwrap(),
        a = serde_json::to_string(attr).unwrap(),
    );
    let res = state.page_mut().evaluate(&js);
    if res.is_null() {
        return Err(format!("Element not found: {selector}"));
    }
    Ok(res.as_str().unwrap_or("").to_string())
}

fn tool_count(args: &Value, state: &mut BrowserState) -> Result<String, String> {
    let selector = args.get("selector").and_then(Value::as_str)
        .ok_or("Missing selector parameter")?;
    let js = format!(
        "document.querySelectorAll({sel}).length",
        sel = serde_json::to_string(selector).unwrap()
    );
    let res = state.page_mut().evaluate(&js);
    // V8 numbers come back as f64 even when they are integer-valued; as_u64
    // returns None for f64 in serde_json, so coerce via f64.
    let n = res.as_u64()
        .or_else(|| res.as_f64().map(|f| f as u64))
        .unwrap_or(0);
    Ok(n.to_string())
}

/// Extract structured data: `schema` is a map of field name to CSS
/// selector. Suffix selector with `@attr` to read an attribute instead
/// of text. Suffix field name with `[]` to return an array (queries all
/// matching elements rather than the first).
fn tool_extract(args: &Value, state: &mut BrowserState) -> Result<String, String> {
    let schema = args.get("schema").and_then(Value::as_object)
        .ok_or("Missing schema object")?
        .clone();
    let schema_json = serde_json::to_string(&schema).unwrap();
    let js = format!(r#"(function(){{
        var schema = {schema};
        var out = {{}};
        for (var key in schema) {{
            if (!Object.prototype.hasOwnProperty.call(schema, key)) continue;
            var spec = schema[key];
            var is_array = key.endsWith('[]');
            var name = is_array ? key.slice(0, -2) : key;
            // Selector may end with `@attr` to read an attribute.
            var attr = null;
            var sel = spec;
            var at = spec.lastIndexOf('@');
            if (at > 0 && spec.indexOf(' ', at) < 0) {{
                attr = spec.slice(at + 1);
                sel = spec.slice(0, at);
            }}
            var get = function(el) {{
                if (!el) return null;
                if (attr) return el.getAttribute(attr) || '';
                return ((el.innerText || el.textContent) || '').trim();
            }};
            if (is_array) {{
                var els = document.querySelectorAll(sel);
                var arr = [];
                for (var i = 0; i < els.length; i++) arr.push(get(els[i]));
                out[name] = arr;
            }} else {{
                out[name] = get(document.querySelector(sel));
            }}
        }}
        return out;
    }})()"#, schema = schema_json);
    let res = state.page_mut().evaluate(&js);
    serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
}

async fn tool_tab_new(args: &Value, state: &mut BrowserState) -> Result<String, String> {
    let url = args.get("url").and_then(Value::as_str);
    let id = state.new_tab();
    if let Some(u) = url {
        let ua = state.user_agent.clone();
        let page = state.page_mut();
        if let Some(ref ua) = ua {
            page.http_client.set_user_agent(ua).await;
        }
        page.navigate_with_wait(u, telemaco_browser::lifecycle::WaitUntil::DomContentLoaded)
            .await.map_err(|e| e.to_string())?;
        Ok(format!("Opened {id} and navigated to {}", page.url_string()))
    } else {
        Ok(format!("Opened {id} (about:blank)."))
    }
}

fn tool_tab_list(state: &BrowserState) -> Result<String, String> {
    if state.tabs.is_empty() {
        return Ok("No tabs open.".to_string());
    }
    let lines: Vec<String> = state.tabs.iter().map(|(id, page)| {
        let active = if Some(id) == state.active_tab.as_ref() { "*" } else { " " };
        let url = page.url_string();
        let title = page.title.replace('\n', " ");
        format!("{active} {id}  {url}  \"{title}\"")
    }).collect();
    Ok(lines.join("\n"))
}

fn tool_tab_switch(args: &Value, state: &mut BrowserState) -> Result<String, String> {
    let tab_id = args.get("tab_id").and_then(Value::as_str)
        .ok_or("Missing tab_id parameter")?;
    if !state.tabs.contains_key(tab_id) {
        return Err(format!("No such tab: {tab_id}"));
    }
    state.active_tab = Some(tab_id.to_string());
    state.interactive_refs.clear();
    Ok(format!("Active tab: {tab_id}"))
}

fn tool_tab_close(args: &Value, state: &mut BrowserState) -> Result<String, String> {
    let tab_id = args.get("tab_id").and_then(Value::as_str)
        .map(String::from)
        .or_else(|| state.active_tab.clone())
        .ok_or("No tab to close")?;
    if !state.close_tab(&tab_id) {
        return Err(format!("No such tab: {tab_id}"));
    }
    if state.active_tab.as_deref() == Some(&tab_id) {
        // Promote some remaining tab to active, if any.
        state.active_tab = state.tabs.keys().next().cloned();
        state.interactive_refs.clear();
    }
    let summary = if let Some(ref a) = state.active_tab {
        format!("Closed {tab_id}. Active tab now {a}.")
    } else {
        format!("Closed {tab_id}. No tabs remain.")
    };
    Ok(summary)
}

fn extract_text(dom: &telemaco_dom::DomTree, node_id: telemaco_dom::NodeId) -> String {
    use telemaco_dom::NodeData;

    let mut result = String::new();
    let node = match dom.get_node(node_id) {
        Some(n) => n,
        None => return result,
    };

    match &node.data {
        NodeData::Text { contents } => {
            let trimmed = contents.trim();
            if !trimmed.is_empty() {
                result.push_str(trimmed);
                result.push(' ');
            }
        }
        NodeData::Element { name, .. } => {
            let tag = name.local.as_ref();
            if matches!(tag, "script" | "style" | "noscript") {
                return result;
            }

            let is_block = matches!(
                tag,
                "div" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                    | "li" | "tr" | "br" | "hr" | "section" | "article"
                    | "header" | "footer" | "nav" | "main" | "aside"
                    | "blockquote" | "pre" | "ul" | "ol" | "table"
            );

            if is_block {
                result.push('\n');
            }

            for child in dom.children(node_id) {
                result.push_str(&extract_text(dom, child));
            }

            if is_block {
                result.push('\n');
            }
        }
        _ => {
            for child in dom.children(node_id) {
                result.push_str(&extract_text(dom, child));
            }
        }
    }

    result
}

// ===== Tier 3 agent-UX additions =====

/// Substring search in visible page text. Returns each match with N chars
/// of surrounding context so the agent can locate the section without
/// pulling the whole page into its window.
fn tool_search(args: &Value, state: &mut BrowserState) -> Result<String, String> {
    let query = args.get("query").and_then(Value::as_str)
        .ok_or("Missing query parameter")?;
    let case_sensitive = args.get("case_sensitive").and_then(Value::as_bool).unwrap_or(false);
    let limit = ExtractionLimits::override_with(
        state.limits.max_search_results,
        args.get("limit").and_then(Value::as_u64),
    );
    let context = ExtractionLimits::override_with(
        state.limits.search_context_chars,
        args.get("context_chars").and_then(Value::as_u64),
    );

    let page = state.page_mut();
    let body = page.with_dom(|dom| {
        dom.query_selector("body").ok().flatten()
            .map(|b| extract_text(dom, b))
            .unwrap_or_default()
    }).unwrap_or_default();

    let haystack = if case_sensitive { body.clone() } else { body.to_lowercase() };
    let needle = if case_sensitive { query.to_string() } else { query.to_lowercase() };

    let mut out = Vec::new();
    let mut idx = 0;
    while let Some(pos) = haystack[idx..].find(&needle) {
        let abs = idx + pos;
        let mut start = abs.saturating_sub(context);
        let mut end = (abs + needle.len() + context).min(body.len());
        // start/end are byte offsets derived from char counts and needle.len(),
        // so they can land inside a multi-byte (CJK) character. Snap to char
        // boundaries before slicing or body[..start] panics (#257).
        while start > 0 && !body.is_char_boundary(start) { start -= 1; }
        while end < body.len() && !body.is_char_boundary(end) { end += 1; }
        // Trim inward to the nearest whitespace so snippets start/end on words.
        if let Some(i) = body[..start].rfind(|c: char| c.is_whitespace()) {
            start = i + body[i..].chars().next().map_or(1, char::len_utf8);
        }
        if let Some(i) = body[end..].find(|c: char| c.is_whitespace()) {
            end += i;
        }
        let snippet = body.get(start..end).unwrap_or("").trim().replace('\n', " ");
        out.push(json!({
            "offset": abs,
            "snippet": snippet,
        }));
        idx = abs + needle.len();
        if out.len() >= limit { break; }
    }
    if out.is_empty() {
        Ok(format!("No matches for {query:?}."))
    } else {
        Ok(format!("{} match(es). {}", out.len(),
            out.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("\n")))
    }
}

/// Export full session state: cookies + localStorage + sessionStorage
/// for every origin the page knows about. Agents stash this between
/// runs to skip a login flow.
fn tool_storage_state(state: &mut BrowserState) -> Result<String, String> {
    let cookies: Vec<Value> = state.context.cookie_jar.get_all_cookies().iter().map(|c| json!({
        "name": c.name,
        "value": c.value,
        "domain": c.domain,
        "path": c.path,
        "secure": c.secure,
        "http_only": c.http_only,
        "same_site": c.same_site,
        "expires": c.expires,
    })).collect();
    // Pull localStorage + sessionStorage for the current page's origin.
    let storage_js = r#"(function(){
        var ls = [], ss = [];
        try { for (var i = 0; i < localStorage.length; i++) { var k = localStorage.key(i); ls.push([k, localStorage.getItem(k)]); } } catch(e) {}
        try { for (var j = 0; j < sessionStorage.length; j++) { var k2 = sessionStorage.key(j); ss.push([k2, sessionStorage.getItem(k2)]); } } catch(e) {}
        return { origin: location.origin || '', localStorage: ls, sessionStorage: ss };
    })()"#;
    let storage = if state.active_tab.is_some() {
        state.page_mut().evaluate(storage_js)
    } else {
        Value::Null
    };
    let origins = if storage.is_object() { vec![storage] } else { vec![] };
    let out = json!({ "cookies": cookies, "origins": origins });
    serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
}

fn tool_set_storage_state(args: &Value, state: &mut BrowserState) -> Result<String, String> {
    let s = args.get("state").ok_or("Missing state object")?;
    let mut applied = 0u32;
    // Cookies
    if let Some(cookies) = s.get("cookies").and_then(Value::as_array) {
        let parsed: Vec<telemaco_net::CookieInfo> = cookies.iter().filter_map(|c| {
            Some(telemaco_net::CookieInfo {
                name: c.get("name")?.as_str()?.to_string(),
                value: c.get("value")?.as_str()?.to_string(),
                domain: c.get("domain")?.as_str()?.to_string(),
                path: c.get("path").and_then(Value::as_str).unwrap_or("/").to_string(),
                secure: c.get("secure").and_then(Value::as_bool).unwrap_or(false),
                http_only: c.get("http_only").and_then(Value::as_bool).unwrap_or(false),
                same_site: c.get("same_site").and_then(Value::as_str).unwrap_or("").to_string(),
                expires: c.get("expires").and_then(Value::as_i64),
            })
        }).collect();
        applied += parsed.len() as u32;
        state.context.cookie_jar.set_cookies_from_cdp(parsed);
    }
    // Storage (per origin). Only applies if there's an active page; we
    // restore on whatever origin is currently loaded, which usually
    // matches because agents navigate before restoring state.
    if state.active_tab.is_some() {
        if let Some(origins) = s.get("origins").and_then(Value::as_array) {
            for origin_entry in origins {
                let mut snippets = Vec::new();
                if let Some(arr) = origin_entry.get("localStorage").and_then(Value::as_array) {
                    for pair in arr {
                        if let (Some(k), Some(v)) = (
                            pair.get(0).and_then(Value::as_str),
                            pair.get(1).and_then(Value::as_str),
                        ) {
                            snippets.push(format!(
                                "try {{ localStorage.setItem({k},{v}); }} catch(e) {{}};",
                                k = serde_json::to_string(k).unwrap(),
                                v = serde_json::to_string(v).unwrap(),
                            ));
                            applied += 1;
                        }
                    }
                }
                if let Some(arr) = origin_entry.get("sessionStorage").and_then(Value::as_array) {
                    for pair in arr {
                        if let (Some(k), Some(v)) = (
                            pair.get(0).and_then(Value::as_str),
                            pair.get(1).and_then(Value::as_str),
                        ) {
                            snippets.push(format!(
                                "try {{ sessionStorage.setItem({k},{v}); }} catch(e) {{}};",
                                k = serde_json::to_string(k).unwrap(),
                                v = serde_json::to_string(v).unwrap(),
                            ));
                            applied += 1;
                        }
                    }
                }
                if !snippets.is_empty() {
                    let _ = state.page_mut().evaluate(&snippets.join("\n"));
                }
            }
        }
    }
    Ok(format!("Restored {applied} state entries."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed_tools() -> Vec<Value> {
        listed_tools_with(&ExtractionLimits::default())
    }

    fn listed_tools_with(limits: &ExtractionLimits) -> Vec<Value> {
        handle_tools_list(json!(1), limits).result.expect("tools/list result")
            .get("tools").and_then(Value::as_array).cloned().expect("tools array")
    }

    /// Helper: the description string of one property on one tool's schema.
    fn property_description(tools: &[Value], tool: &str, property: &str) -> String {
        tools
            .iter()
            .find(|t| t.get("name").and_then(Value::as_str) == Some(tool))
            .unwrap_or_else(|| panic!("tool {tool} not advertised"))
            .pointer(&format!("/inputSchema/properties/{property}/description"))
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{tool}.{property} has no description"))
            .to_string()
    }

    #[test]
    fn advertised_defaults_follow_the_configured_limits() {
        let limits = ExtractionLimits { max_chars: 25_000, max_links: 7, ..Default::default() };
        let tools = listed_tools_with(&limits);

        assert!(
            property_description(&tools, "browser_markdown", "max_chars").contains("25000"),
            "markdown schema must advertise the configured cap, not a hardcoded one"
        );
        assert!(
            property_description(&tools, "browser_links", "limit").contains('7'),
            "links schema must advertise the configured cap"
        );
    }

    #[test]
    fn advertised_default_says_unlimited_for_zero() {
        let limits = ExtractionLimits { max_chars: 0, ..Default::default() };
        let tools = listed_tools_with(&limits);
        let described = property_description(&tools, "browser_markdown", "max_chars");
        assert!(
            described.contains("unlimited"),
            "0 is the unlimited sentinel and must not leak into the schema, got: {described}"
        );
    }

    #[test]
    fn default_limits_still_advertise_the_historical_values() {
        // Guards the inert-by-default promise: with no config, clients see
        // exactly what they saw before the limits became configurable.
        let tools = listed_tools();
        assert!(property_description(&tools, "browser_markdown", "max_chars").contains("4000"));
        assert!(property_description(&tools, "browser_links", "limit").contains("100"));
    }

    #[test]
    fn omitted_note_is_silent_when_nothing_was_cut() {
        assert_eq!(omitted_note(10, 10), "");
        assert_eq!(omitted_note(10, 3), "");
    }

    #[test]
    fn omitted_note_reports_the_shortfall_and_the_total() {
        let note = omitted_note(2, 7);
        assert!(note.contains('5'), "should say how many were dropped: {note}");
        assert!(note.contains('7'), "should say the total: {note}");
        assert!(
            note.contains("raise the cap"),
            "should tell the agent how to get the rest: {note}"
        );
    }

    // --- adaptation of the pagination work onto configurable limits ---

    /// A state whose limits differ from the defaults, with the markdown cache
    /// pre-seeded so the paginated path runs without V8.
    fn seeded_state_with(url: &str, md: &str, limits: ExtractionLimits) -> BrowserState {
        let mut state = BrowserState::new(None, None, false, limits);
        let _ = state.page_mut();
        state.markdown_cache = Some((url.to_string(), md.to_string()));
        state
    }

    #[test]
    fn configured_cap_drives_pagination_with_no_tool_argument() {
        // The whole point of the adaptation: with no `max_chars` in the call,
        // the page size must come from the resolved config rather than from a
        // constant. A small configured cap has to force a multi-page document.
        let doc = "alpha bravo\n\ncharlie delta\n\necho foxtrot\n\ngolf hotel\n";
        let limits = ExtractionLimits { max_chars: 20, ..Default::default() };
        let mut state = seeded_state_with("about:blank", doc, limits);

        let page1 = tool_markdown(&json!({}), &mut state).expect("page 1");
        let total = page_total(&page1);
        assert!(total > 1, "a 20-character cap must split this document, got {total} page(s)");
        assert!(!page1.contains("golf hotel"), "page 1 must not carry the whole document");
    }

    #[test]
    fn unlimited_configured_cap_returns_the_whole_document_in_one_page() {
        // 0 is the unlimited sentinel everywhere else, and pagination has to
        // agree with it: one page holding everything, not the one-character
        // page a naive `.max(1)` clamp would produce.
        let doc = "alpha bravo\n\ncharlie delta\n\necho foxtrot\n\ngolf hotel\n";
        let limits = ExtractionLimits { max_chars: 0, ..Default::default() };
        let mut state = seeded_state_with("about:blank", doc, limits);

        let out = tool_markdown(&json!({}), &mut state).expect("page 1");
        assert!(out.contains("alpha bravo") && out.contains("golf hotel"));
        assert!(out.contains("page 1 of 1"), "unlimited must yield exactly one page: {out}");
    }

    #[test]
    fn packing_never_panics_or_loses_multibyte_text() {
        // The upstream `.max(1)` clamp guarded split_long_line from slicing at
        // byte 1 of a multi-byte character. Routing the cap through
        // ExtractionLimits keeps a zero from reaching that path, and small
        // non-zero caps must stay safe too. Every character has to survive.
        let doc = "pero\u{300} \u{c8} accentata u\u{308}ni\u{308}co\u{308}de\u{308} \u{65e5}\u{672c}\u{8a9e}\u{306e}\u{30c6}\u{30ad}\u{30b9}\u{30c8}\n\nsecondo blocco a\u{300}e\u{300}i\u{300}o\u{300}u\u{300}\n";
        let expected: String = doc.chars().filter(|c| !c.is_whitespace()).collect();

        for raw in [0usize, 1, 3, 10, 4_000] {
            let pages = blocks::pack_pages(doc, ExtractionLimits::cap(raw));
            let got: String = pages.concat().chars().filter(|c| !c.is_whitespace()).collect();
            assert_eq!(
                got, expected,
                "cap {raw} lost or corrupted characters while packing"
            );
        }
    }

    #[test]
    fn markdown_schema_advertises_the_configured_page_size() {
        let limits = ExtractionLimits { max_chars: 12_345, ..Default::default() };
        let described = property_description(&listed_tools_with(&limits), "browser_markdown", "max_chars");
        assert!(
            described.contains("12345"),
            "the paginated schema must still report the resolved cap: {described}"
        );
        assert!(
            described.contains("Blocks are never cut in half"),
            "the pagination wording must survive the adaptation: {described}"
        );
    }

    // browser_markdown pagination against a manually-seeded cache: the
    // cache-hit path never calls page.evaluate, so no V8 is needed.
    fn seeded_state(url: &str, md: &str) -> BrowserState {
        let mut state = BrowserState::new(None, None, false, ExtractionLimits::default());
        // Touch page_mut so the active tab exists and url_string() is set.
        let _ = state.page_mut();
        state.markdown_cache = Some((url.to_string(), md.to_string()));
        state
    }

    fn tool_markdown_page(state: &mut BrowserState, page: u64, max_chars: usize) -> String {
        tool_markdown(&json!({ "page": page, "max_chars": max_chars }), state)
            .expect("tool_markdown ok")
    }

    #[test]
    fn browser_markdown_serves_cached_pages_without_reconversion() {
        let doc = "alpha\n\nbeta\n\ngamma\n\ndelta\n";
        let mut state = seeded_state("about:blank", doc);
        // Small ceiling forces multiple pages from the cached document.
        let p1 = tool_markdown_page(&mut state, 1, 12);
        assert!(p1.contains("alpha"));
        assert!(p1.contains("page 1 of"), "marker missing: {p1}");
        let p2 = tool_markdown_page(&mut state, 2, 12);
        assert!(p2.contains("beta"), "p2: {p2}");
        assert!(p2.contains("page 2 of 4"), "p2: {p2}");
        // Cache untouched: still the same document.
        assert_eq!(state.markdown_cache.as_ref().unwrap().1, doc);
    }

    #[test]
    fn browser_markdown_last_page_says_end() {
        let doc = "alpha\n\nbeta\n\ngamma\n\ndelta\n";
        let mut state = seeded_state("about:blank", doc);
        let (_, total) = blocks::paginate(doc, 12, 1);
        let last = tool_markdown_page(&mut state, total as u64, 12);
        assert!(last.contains("end of document"));
        assert!(last.contains(&format!("page {total} of {total}")));
    }

    #[test]
    fn browser_markdown_beyond_end_is_explicit() {
        let doc = "alpha\n\nbeta\n\ngamma\n\ndelta\n";
        let mut state = seeded_state("about:blank", doc);
        let (_, total) = blocks::paginate(doc, 12, 1);
        let out = tool_markdown_page(&mut state, total as u64 + 5, 12);
        assert!(out.contains("beyond the end"));
        assert!(out.contains(&format!("{total} page(s) total")));
    }

    #[test]
    fn browser_markdown_url_change_invalidates_cache() {
        let doc = "alpha\n\nbeta\n\ngamma\n\ndelta\n";
        let mut state = seeded_state("about:blank", doc);
        // Active tab URL differs from the cached URL: the cached document
        // must not be served; re-conversion of a blank page yields "".
        state.markdown_cache = Some(("https://other.example".to_string(), doc.to_string()));
        let out = tool_markdown(&json!({ "page": 1 }), &mut state).unwrap();
        assert!(out.contains("empty page"));
        // The empty conversion replaced the stale cache entry.
        let (url, md) = state.markdown_cache.as_ref().unwrap();
        assert_eq!(url, "about:blank");
        assert_eq!(md, "");
    }

    #[test]
    fn browser_markdown_refresh_forces_reconversion() {
        let doc = "alpha\n\nbeta\n\ngamma\n\ndelta\n";
        let mut state = seeded_state("about:blank", doc);
        let out = tool_markdown(&json!({ "page": 1, "refresh": true }), &mut state).unwrap();
        // Blank page converts to nothing; cache replaced with "".
        assert!(out.contains("empty page"));
        assert_eq!(state.markdown_cache.as_ref().unwrap().1, "");
    }

    #[test]
    fn browser_markdown_schema_documents_pagination() {
        let tools = listed_tools();
        let md = tools.iter().find(|tool| tool["name"] == "browser_markdown")
            .expect("browser_markdown tool");
        assert!(md["inputSchema"]["properties"]["page"].is_object());
        assert!(md["inputSchema"]["properties"]["refresh"].is_object());
        assert!(md["description"].as_str().unwrap().contains("paginated"));
    }

    #[test]
    fn tool_schemas_expose_snapshot_limit_without_nested_properties() {
        let tools = listed_tools();
        let snapshot = tools.iter().find(|tool| tool["name"] == "browser_snapshot")
            .expect("browser_snapshot tool");
        assert_eq!(snapshot["inputSchema"]["properties"]["max_chars"]["type"], "number");
        for tool in tools {
            assert!(
                tool["inputSchema"]["properties"].get("properties").is_none(),
                "{} has a nested duplicate properties object", tool["name"]
            );
        }
    }

    #[cfg(not(feature = "render"))]
    #[test]
    fn render_tools_are_not_advertised_without_render_feature() {
        let tools = listed_tools();
        assert!(tools.iter().all(|tool| {
            tool["name"] != "browser_screenshot" && tool["name"] != "browser_pdf"
        }));
    }

    #[cfg(feature = "render")]
    #[test]
    fn render_tools_are_advertised_with_flat_schemas() {
        let tools = listed_tools();
        for name in ["browser_screenshot", "browser_pdf"] {
            let tool = tools.iter().find(|tool| tool["name"] == name)
                .unwrap_or_else(|| panic!("missing {name}"));
            assert_eq!(tool["inputSchema"]["type"], "object");
            assert_eq!(tool["inputSchema"]["additionalProperties"], false);
            assert!(tool["inputSchema"]["properties"].is_object());
            assert!(tool["inputSchema"]["properties"].get("properties").is_none());
        }
    }

    #[cfg(feature = "render")]
    #[tokio::test(flavor = "current_thread")]
    async fn render_tool_calls_return_mcp_binary_content_and_reject_bad_options() {
        let mut state = BrowserState::new(None, None, false, ExtractionLimits::default());
        state.page_mut().navigate(
            "data:text/html,<html style='margin:0'><body style='margin:0;background:red'><div style='width:64px;height:48px'></div></body></html>",
        ).await.expect("render test page should navigate");
        state.page_mut().set_viewport((64.0, 48.0));

        let screenshot = handle_tool_call(
            json!(1), &json!({ "name": "browser_screenshot", "arguments": {} }), &mut state,
        ).await.result.expect("screenshot response");
        let image = &screenshot["content"][0];
        assert_eq!(image["type"], "image");
        assert_eq!(image["mimeType"], "image/png");
        let png = BASE64.decode(image["data"].as_str().expect("PNG base64"))
            .expect("valid PNG base64");
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));

        let pdf = handle_tool_call(
            json!(2),
            &json!({ "name": "browser_pdf", "arguments": { "print_background": true } }),
            &mut state,
        ).await.result.expect("PDF response");
        let resource = &pdf["content"][0];
        assert_eq!(resource["type"], "resource");
        assert_eq!(resource["resource"]["mimeType"], "application/pdf");
        let bytes = BASE64.decode(resource["resource"]["blob"].as_str().expect("PDF base64"))
            .expect("valid PDF base64");
        assert!(bytes.starts_with(b"%PDF-"));

        let invalid_screenshot = handle_tool_call(
            json!(3),
            &json!({ "name": "browser_screenshot", "arguments": { "width": 0 } }),
            &mut state,
        ).await.result.expect("invalid screenshot response");
        assert_eq!(invalid_screenshot["isError"], true);

        let invalid_pdf = handle_tool_call(
            json!(4),
            &json!({ "name": "browser_pdf", "arguments": { "scale": 3 } }),
            &mut state,
        ).await.result.expect("invalid PDF response");
        assert_eq!(invalid_pdf["isError"], true);
    }

    /// Records the method, path and body of every request it serves, so a test
    /// can assert what actually left the process rather than what the page
    /// believes happened. Serves the issue's form at `/` and 200s everything
    /// else.
    fn spawn_form_recording_server() -> (String, std::sync::mpsc::Receiver<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut buf = [0u8; 8192];
                let read = stream.read(&mut buf).unwrap_or(0);
                let raw = String::from_utf8_lossy(&buf[..read]).to_string();
                let start = raw.lines().next().unwrap_or("").to_string();
                let mut parts = start.split_whitespace();
                let method = parts.next().unwrap_or("").to_string();
                let path = parts.next().unwrap_or("").to_string();
                let body = raw.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
                let _ = tx.send(format!("{method} {path} body='{body}'"));
                let page = "<!doctype html><meta charset=utf-8>\
                    <form id=f method=POST action=/submitted>\
                    <input name=q id=q value=><button type=submit id=go>Envoyer</button></form>";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    page.len(),
                    page
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        (base, rx)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn click_on_a_submit_button_issues_the_request_before_replying() {
        // A submit click runs the page's form glue and leaves the navigation as
        // pending state; it only becomes a request when a driving layer converts
        // it. The CDP path converts it, MCP did not, so the same click POSTed
        // over CDP and did nothing over MCP (#618). Worse than doing nothing:
        // `location.href` already reported the destination, so an agent was told
        // the submit had happened while no request had left the process.
        //
        // The assertion is deliberately at the wire, not on `location.href`,
        // because the URL was the thing that lied.
        // nextest gives each test its own process, so this cannot reach a
        // sibling. The recording server is on 127.0.0.1, which the SSRF gate
        // refuses by default, exactly as the issue reporter had to pass
        // --allow-private-network to run their repro.
        std::env::set_var("TELEMACO_ALLOW_PRIVATE_NETWORK", "1");
        let (base, requests) = spawn_form_recording_server();
        let mut state = BrowserState::new(None, None, false, ExtractionLimits::default());
        state
            .page_mut()
            .navigate(&base)
            .await
            .expect("form page should navigate");
        assert!(
            requests
                .recv_timeout(std::time::Duration::from_secs(2))
                .expect("the form page itself must be fetched")
                .starts_with("GET /"),
        );

        tool_fill(&json!({ "selector": "#q", "value": "hello" }), &mut state)
            .await
            .expect("browser_fill should succeed");
        tool_click(&json!({ "selector": "#go" }), &mut state)
            .await
            .expect("browser_click should succeed");

        let submitted = requests
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("the submit must reach the server before browser_click replies");
        assert!(
            submitted.starts_with("POST /submitted"),
            "expected a POST to the form action, got {submitted}"
        );
        assert!(
            submitted.contains("q=hello"),
            "the submitted body must carry the filled field, got {submitted}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fill_tools_notify_controlled_input_tracker() {
        let mut state = BrowserState::new(None, None, false, ExtractionLimits::default());
        state
            .page_mut()
            .navigate("data:text/html,<div id=root><input id=field></div>")
            .await
            .expect("test page should navigate");

        state.page_mut().evaluate(
            r#"(function () {
                var root = document.getElementById('root');
                var input = document.getElementById('field');
                var descriptor = Object.getOwnPropertyDescriptor(input.constructor.prototype, 'value');
                var tracked = String(input.value);
                Object.defineProperty(input, 'value', {
                    configurable: true,
                    get: function () { return descriptor.get.call(this); },
                    set: function (value) {
                        tracked = String(value);
                        descriptor.set.call(this, value);
                    }
                });
                window.__controlledState = '';
                window.__controlledUpdates = 0;
                window.__lastInputTarget = '';
                window.__lastInputTrusted = false;
                root.addEventListener('input', function (event) {
                    window.__lastInputTarget = event.target.id;
                    window.__lastInputTrusted = event.isTrusted;
                    var next = String(event.target.value);
                    if (next !== tracked) {
                        tracked = next;
                        window.__controlledState = next;
                        window.__controlledUpdates++;
                    }
                });
            })()"#,
        );

        tool_fill(
            &json!({ "selector": "#field", "value": "filled" }),
            &mut state,
        )
        .await
        .expect("browser_fill should succeed");
        tool_type(
            &json!({ "selector": "#field", "text": "-typed" }),
            &mut state,
        )
        .await
        .expect("browser_type should succeed");
        tool_fill_form(
            &json!({
                "fields": [{ "selector": "#field", "value": "form-filled" }]
            }),
            &mut state,
        )
        .expect("browser_fill_form should succeed");

        let actual = state.page_mut().evaluate(
            r#"JSON.stringify({
                domValue: document.getElementById('field').value,
                controlledState: window.__controlledState,
                controlledUpdates: window.__controlledUpdates,
                lastInputTarget: window.__lastInputTarget,
                lastInputTrusted: window.__lastInputTrusted
            })"#,
        );
        assert_eq!(
            actual,
            json!(r#"{"domValue":"form-filled","controlledState":"form-filled","controlledUpdates":3,"lastInputTarget":"field","lastInputTrusted":true}"#),
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fill_form_check_and_select_use_native_setter_and_trusted_events() {
        let mut state = BrowserState::new(None, None, false, ExtractionLimits::default());
        state
            .page_mut()
            .navigate(
                "data:text/html,<div id=root><input id=box type=checkbox>\
                 <select id=sel><option value=a>a</option><option value=b>b</option></select></div>",
            )
            .await
            .expect("test page should navigate");

        // Install a React-style tracker on the checkbox's `checked`, redefined on
        // the instance. A direct `el.checked = true` runs this wrapper in lockstep,
        // so a change handler comparing target.checked to the tracked value sees no
        // change and never commits. Writing through the prototype setter (what
        // __telemaco_setFieldValue does) leaves the tracker stale so the edit
        // registers. Also record whether the dispatched change is trusted.
        state.page_mut().evaluate(
            r#"(function () {
                var box = document.getElementById('box');
                var root = document.getElementById('root');
                var d = Object.getOwnPropertyDescriptor(box.constructor.prototype, 'checked');
                var tracked = box.checked;
                Object.defineProperty(box, 'checked', {
                    configurable: true,
                    get: function () { return d.get.call(this); },
                    set: function (v) { tracked = !!v; d.set.call(this, v); }
                });
                window.__checkedCommitted = false;
                window.__checkTrusted = false;
                window.__selectTrusted = false;
                root.addEventListener('change', function (event) {
                    if (event.target.id === 'box') {
                        window.__checkTrusted = event.isTrusted;
                        if (event.target.checked !== tracked) {
                            tracked = event.target.checked;
                            window.__checkedCommitted = true;
                        }
                    } else if (event.target.id === 'sel') {
                        window.__selectTrusted = event.isTrusted;
                    }
                });
            })()"#,
        );

        tool_fill_form(
            &json!({
                "fields": [
                    { "selector": "#box", "type": "check" },
                    { "selector": "#sel", "type": "select", "value": "b" }
                ]
            }),
            &mut state,
        )
        .expect("browser_fill_form should succeed");

        let actual = state.page_mut().evaluate(
            r#"JSON.stringify({
                domChecked: document.getElementById('box').checked,
                checkedCommitted: window.__checkedCommitted,
                checkTrusted: window.__checkTrusted,
                selValue: document.getElementById('sel').value,
                selectTrusted: window.__selectTrusted
            })"#,
        );
        assert_eq!(
            actual,
            json!(r#"{"domChecked":true,"checkedCommitted":true,"checkTrusted":true,"selValue":"b","selectTrusted":true}"#),
        );
    }

    // Real-site pagination checks for browser_markdown. Ignored by default:
    // needs network and a full render. Run with
    // `cargo test -p telemaco-mcp --lib -- --ignored --nocapture`.
    //
    // The two tests pin the two agent-facing scenarios:
    //   - Salesforce: the needed sentence (503 rate-limit note) sits in the
    //     first ~4000 chars. Page 1 is sufficient; page 2 carries only the
    //     OneTrust cookie banner. An agent can stop after page 1.
    //   - docs.rs: the Serde overview prose is split at the first page
    //     boundary ("usage examples." ends page 1; the reflection/trait
    //     paragraphs start page 2). The agent MUST read page 2 to get the
    //     rest, and pagination must hand it over losslessly.
    //
    // Both also verify page>=2 is served from the cache, not re-converted.
    async fn navigate_to(url: &str, state: &mut BrowserState) {
        let page = state.page_mut();
        page.set_navigation_timeout(std::time::Duration::from_secs(60));
        page.navigate_with_wait(url, telemaco_browser::lifecycle::WaitUntil::from_str("load"))
            .await
            .expect("navigate");
        // SPA content mounts after load; same settle the CLI does.
        page.settle(5000).await;
    }

    fn page_total(marker_page: &str) -> usize {
        marker_page.split("page 1 of ")
            .nth(1)
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| s.parse::<usize>().ok())
            .expect("page 1 carries a total page count")
    }

    #[tokio::test(flavor = "current_thread")]
    #[ignore = "requires network and full render"]
    async fn salesforce_page1_suffices_and_page2_is_banner_noise() {
        const URL: &str = "https://developer.salesforce.com/docs/atlas.en-us.chatterapi.meta/chatterapi/intro_what_is_chatter_connect.htm";
        let mut state = BrowserState::new(None, None, false, ExtractionLimits::default());
        navigate_to(URL, &mut state).await;

        let t0 = std::time::Instant::now();
        let p1 = tool_markdown(&json!({ "page": 1 }), &mut state).expect("page 1");
        let convert_elapsed = t0.elapsed();
        let current_url = state.page_mut().url_string();
        assert!(
            state.markdown_cache.as_ref().expect("cache seeded").0 == current_url,
            "cache keyed by page URL"
        );

        let total = page_total(&p1);
        assert!(total >= 2, "expected multi-page document, got {total}");

        // THE SUFFICIENT CASE: everything needed is on page 1.
        assert!(
            p1.contains("503 Service Unavailable"),
            "the 503 rate-limit note must be on page 1 (agent can stop here)"
        );

        // Page 2 exists (marker says so) and is a pure cache hit.
        let t1 = std::time::Instant::now();
        let p2 = tool_markdown(&json!({ "page": 2 }), &mut state).expect("page 2");
        let cached_elapsed = t1.elapsed();
        assert!(
            cached_elapsed < convert_elapsed / 20,
            "page 2 must be a cache hit: convert {:?} vs cached {:?}",
            convert_elapsed,
            cached_elapsed
        );
        assert!(p2.contains("page 2 of"), "p2 marker missing: {p2}");
        // Page 2 on this page is the cookie banner: real page, no content.
        assert!(
            p2.contains("Cookie Consent Manager"),
            "page 2 should carry the banner noise an agent can ignore"
        );
        println!(
            "salesforce: {total} pages, convert {:?}, cached {:?}, page 2 = banner noise (agent stops at page 1)",
            convert_elapsed,
            cached_elapsed
        );
    }

    #[tokio::test(flavor = "current_thread")]
    #[ignore = "requires network and full render"]
    async fn docs_rs_page2_carries_the_core_content() {
        const URL: &str = "https://docs.rs/serde/latest/serde/";
        let mut state = BrowserState::new(None, None, false, ExtractionLimits::default());
        navigate_to(URL, &mut state).await;

        let p1 = tool_markdown(&json!({ "page": 1 }), &mut state).expect("page 1");
        let total = page_total(&p1);
        assert!(total >= 2, "expected multi-page document, got {total}");

        // THE MUST-CONTINUE CASE: the overview prose continues past the
        // first page boundary. Page 1 ends after "usage examples."; the
        // reflection/trait-system paragraphs live on page 2.
        assert!(
            !p1.contains("Where many other languages rely on runtime reflection"),
            "the continuation prose must NOT be on page 1 for this fixture"
        );

        let t0 = std::time::Instant::now();
        let p2 = tool_markdown(&json!({ "page": 2 }), &mut state).expect("page 2");
        let cached_elapsed = t0.elapsed();
        assert!(
            p2.contains("Where many other languages rely on runtime reflection"),
            "the continuation prose must be reachable on page 2"
        );
        assert!(p2.contains("page 2 of"), "p2 marker missing: {p2}");
        println!(
            "docs.rs: {total} pages, p1 {} chars, continuation on page 2, read {:?} (cache hit)",
            p1.len(),
            cached_elapsed
        );
    }

    // Local-fixture E2E: a page whose paragraph is a single unbreakable
    // ~13k-char markdown line plus a large code fence. Pins the ceiling
    // guarantee (no page ever exceeds max_chars), losslessness across
    // pages, and the cache-hit latency for pages >= 2.
    #[tokio::test(flavor = "current_thread")]
    #[ignore = "requires a local http.server on 127.0.0.1:8931"]
    async fn local_giant_blocks_respect_ceiling_and_losslessness() {
        std::env::set_var("TELEMACO_ALLOW_PRIVATE_NETWORK", "1");
        const URL: &str = "http://127.0.0.1:8931/site/giant.html";
        let mut state = BrowserState::new(None, None, false, ExtractionLimits::default());
        navigate_to(URL, &mut state).await;

        let p1 = tool_markdown(&json!({ "page": 1 }), &mut state).expect("page 1");
        let total = page_total(&p1);
        assert!(total >= 3, "giant fixture should span >= 3 pages, got {total}");

        let mut whole = String::new();
        for page_no in 1..=total {
            let out = tool_markdown(&json!({ "page": page_no }), &mut state)
                .unwrap_or_else(|e| panic!("page {page_no}: {e}"));
            // Ceiling: body (marker excluded) never exceeds max_chars.
            let body = out.split("\n\n<!-- markdown page").next().unwrap_or(&out);
            assert!(
                body.chars().count() <= 4000,
                "page {page_no} exceeds the ceiling: {} chars",
                body.len()
            );
            if !whole.is_empty() {
                whole.push_str("\n\n");
            }
            whole.push_str(out.trim_end());
        }

        // Losslessness: head, tail, and both fence ends survive.
        assert!(whole.contains("HEAD-SENTENCE-START"), "paragraph head lost");
        assert!(whole.contains("TAIL-SENTENCE-END"), "paragraph tail lost");
        assert!(whole.contains("TOKEN-FENCE-HEAD"), "fence head lost");
        assert!(whole.contains("TOKEN-FENCE-TAIL"), "fence tail lost");
        println!("giant fixture: {total} pages, whole {} chars", whole.len());
    }

    // Local-fixture E2E: navigation must invalidate or re-key the cache:
    // browser_markdown after navigating elsewhere must never serve the
    // previous page's cached document.
    #[tokio::test(flavor = "current_thread")]
    #[ignore = "requires a local http.server on 127.0.0.1:8931"]
    async fn local_navigation_rekeys_markdown_cache() {
        std::env::set_var("TELEMACO_ALLOW_PRIVATE_NETWORK", "1");
        const A: &str = "http://127.0.0.1:8931/site/article.html";
        const B: &str = "http://127.0.0.1:8931/site/giant.html";
        let mut state = BrowserState::new(None, None, false, ExtractionLimits::default());

        navigate_to(A, &mut state).await;
        let md_a = tool_markdown(&json!({ "page": 1 }), &mut state).expect("md A");
        assert!(md_a.contains("Rust ownership rules"), "page A content: {md_a}");

        navigate_to(B, &mut state).await;
        let md_b = tool_markdown(&json!({ "page": 1 }), &mut state).expect("md B");
        assert!(md_b.contains("Giant Block Fixture"), "page B content: {md_b}");
        assert!(
            !md_b.contains("Rust ownership rules"),
            "stale page A content leaked into page B"
        );

        // Back to A: cache was keyed to B (or cleared), must re-convert.
        navigate_to(A, &mut state).await;
        let md_a2 = tool_markdown(&json!({ "page": 1 }), &mut state).expect("md A again");
        assert!(md_a2.contains("Rust ownership rules"), "page A re-navigation: {md_a2}");
        assert!(!md_a2.contains("Giant Block Fixture"), "stale page B leaked back");
    }

    // Real-site E2E: two tabs on different sites, markdown interleaved.
    // The cache is keyed by the ACTIVE tab's URL: switching tabs must
    // re-key, and repeated interleaved reads must always return the right
    // site's content, never a cross-contaminated document.
    #[tokio::test(flavor = "current_thread")]
    #[ignore = "requires network and full render"]
    async fn tab_interleaved_markdown_never_cross_contaminates() {
        std::env::set_var("TELEMACO_ALLOW_PRIVATE_NETWORK", "1");
        let mut state = BrowserState::new(None, None, false, ExtractionLimits::default());

        // Tab 1: example.com (single-page doc).
        navigate_to("https://example.com", &mut state).await;
        let md_example = tool_markdown(&json!({ "page": 1 }), &mut state).expect("example.com");
        assert!(md_example.contains("Example Domain"));
        assert!(md_example.contains("end of document"), "example.com fits one page");

        // Tab 2: docs.rs (multi-page doc) via a new tab.
        let _tab2 = tool_tab_new(&json!({}), &mut state).await.expect("tab 2");
        navigate_to("https://docs.rs/serde/latest/serde/", &mut state).await;
        let md_docs = tool_markdown(&json!({ "page": 1 }), &mut state).expect("docs.rs");
        assert!(md_docs.contains("serde"), "docs.rs content: {md_docs}");

        // Switch back to tab 1: content must be example.com again.
        tool_tab_switch(&json!({ "tab_id": "tab-1" }), &mut state).expect("switch to tab-1");
        let md_example2 = tool_markdown(&json!({ "page": 1 }), &mut state).expect("example again");
        assert!(md_example2.contains("Example Domain"));
        assert!(
            !md_example2.contains("serde"),
            "docs.rs content leaked into example.com tab"
        );
    }
}
