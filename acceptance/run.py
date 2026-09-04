#!/usr/bin/env python3
"""Behavioral acceptance gate for telemaco.

Drives the release binary against local fixtures and reports one pass/fail
plus a median latency per stage. Everything is served from this machine, so a
run is deterministic and needs no network.

    TELEMACO_BIN=./target/release/telemaco python3 acceptance/run.py
    TELEMACO_BIN=./target/release/telemaco python3 acceptance/run.py --json

The JSON shape is the contract the CI comparison script consumes:

    {"results": [{"name": str, "pass": bool, "median_ms": float}, ...]}

Stage names are part of that contract. Renaming one makes the comparison
report a missing stage rather than a regression, so treat them as fixed.
"""

from __future__ import annotations

import argparse
import http.server
import json
import os
import re
import shutil
import socket
import statistics
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent
FIXTURES = ROOT / "fixtures"
BIN = os.environ.get("TELEMACO_BIN", str(ROOT.parent / "target" / "release" / "telemaco"))

# Fixtures live on loopback, which the SSRF gate blocks by default.
BASE_ENV = {**os.environ, "TELEMACO_ALLOW_PRIVATE_NETWORK": "1"}


# --------------------------------------------------------------- fixture server


class Handler(http.server.SimpleHTTPRequestHandler):
    """Serves fixtures/ plus the few dynamic endpoints the stages need."""

    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(FIXTURES), **kwargs)

    def log_message(self, *args):  # keep the runner output clean
        pass

    def do_GET(self):
        path = self.path.split("?", 1)[0]
        if path == "/redirect1":
            return self._redirect("/redirect2")
        if path == "/redirect2":
            return self._redirect("/redirect3")
        if path == "/redirect3":
            return self._redirect("/page2.html")
        if path == "/slow-response":
            # Slower than the timeouts the timeout stage uses.
            time.sleep(8)
            return self._plain("late")
        if path == "/gone":
            self.send_error(404, "Not Found")
            return
        return super().do_GET()

    def _redirect(self, target: str) -> None:
        self.send_response(302)
        self.send_header("Location", target)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def _plain(self, body: str) -> None:
        raw = body.encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)


class Fixtures:
    """The fixture server, on a port picked at runtime.

    A fixed port is a trap: a stale server left over from an earlier run answers
    instead, every stage fails, and the cause looks like the engine.
    """

    def __init__(self) -> None:
        self.httpd = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.port = self.httpd.server_address[1]
        self.url = f"http://127.0.0.1:{self.port}"
        self.thread = threading.Thread(target=self.httpd.serve_forever, daemon=True)

    def __enter__(self) -> "Fixtures":
        self.thread.start()
        return self

    def __exit__(self, *exc) -> None:
        self.httpd.shutdown()
        self.httpd.server_close()


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


# ------------------------------------------------------------------- CLI helper


class Run:
    __slots__ = ("rc", "out", "err")

    def __init__(self, rc: int, out: str, err: str) -> None:
        self.rc, self.out, self.err = rc, out, err

    def __contains__(self, needle: str) -> bool:
        return needle in self.out


def cli(*args: str, timeout: float = 60, env: dict | None = None, raw: bool = False):
    """Run the binary with the SSRF gate open, returning captured output."""
    proc = subprocess.run(
        [BIN, *args],
        capture_output=True,
        timeout=timeout,
        env=env or BASE_ENV,
    )
    if raw:
        return proc
    return Run(
        proc.returncode,
        proc.stdout.decode("utf-8", "replace"),
        proc.stderr.decode("utf-8", "replace"),
    )


# ------------------------------------------------------------------- the stages
#
# Each takes the fixture base URL and returns True when the behavior holds.
# Keep every assertion about observable behavior, never about wording that the
# CLI is free to change.


def dom_malformed(u):
    r = cli("fetch", f"{u}/malformed.html", "-q", "--dump", "text")
    return "unclosed paragraph" in r and "MALFORMED-CELL-B" in r


def dom_entities(u):
    r = cli("fetch", f"{u}/entities.html", "-q", "--dump", "text")
    return "ENT-START" in r and "ENT-END" in r and "&" in r and "€" in r


def dom_charset(u):
    r = cli("fetch", f"{u}/latin1.html", "-q", "--dump", "text")
    return "LATIN1:" in r and "èéò" in r


def dom_base_href(u):
    r = cli("fetch", f"{u}/basehref.html", "-q", "--dump", "links")
    return "/sub/target.html" in r


def dom_nested_tables(u):
    r = cli("fetch", f"{u}/nested-tables.html", "-q", "--dump", "text")
    return "outer-a" in r and "INNER-CELL" in r


def js_dom_injection(u):
    r = cli("fetch", f"{u}/index.html", "-q", "--dump", "text")
    return "INJECTED-BY-JS" in r


def js_timers(u):
    r = cli("fetch", f"{u}/timers.html", "-q", "--dump", "text")
    return "TIMER-FIRED" in r


def js_fetch(u):
    r = cli("fetch", f"{u}/fetch.html", "-q", "--dump", "text")
    return "FETCH:OK" in r


def js_xhr(u):
    r = cli("fetch", f"{u}/xhr.html", "-q", "--dump", "text")
    return "XHR:OK" in r


def js_async_await(u):
    r = cli("fetch", f"{u}/async.html", "-q", "--dump", "text")
    return "ASYNC:RESOLVED" in r


def js_custom_elements(u):
    r = cli("fetch", f"{u}/custom-element.html", "-q", "--dump", "text")
    return "CUSTOM-ELEMENT-UPGRADED" in r


def js_dom_mutation(u):
    # AGENTS.md flags insertBefore and replaceChild as easy to break, because
    # the reference-node and parent arguments are simple to swap. A swap still
    # produces a list, just in the wrong order, so the order is the assertion.
    # Expected by hand from the DOM spec, not copied from current behavior.
    r = cli("fetch", f"{u}/mutation.html", "-q", "--dump", "text")
    return "ORDER:BEFORE,REPLACED,INSERTED,AFTER,REPLACEDWITH" in r


def extract_text(u):
    r = cli("fetch", f"{u}/index.html", "-q", "--dump", "text")
    return "Acceptance root" in r and "<h1" not in r


def extract_html(u):
    r = cli("fetch", f"{u}/index.html", "-q", "--dump", "html")
    return "<h1" in r and 'id="injected"' in r


def extract_links(u):
    r = cli("fetch", f"{u}/index.html", "-q", "--dump", "links")
    return "/page2.html" in r and "example.invalid" in r


def extract_markdown(u):
    r = cli("fetch", f"{u}/index.html", "-q", "--dump", "markdown")
    return "# Acceptance root" in r and "](" in r


def extract_original_binary(u):
    proc = cli("fetch", f"{u}/pixel.png", "-q", "--dump", "original", raw=True)
    return proc.stdout == (FIXTURES / "pixel.png").read_bytes()


def extract_assets(u):
    r = cli("fetch", f"{u}/assets.html", "-q", "--dump", "assets")
    return "style.css" in r and "pixel.png" in r and "script.js" in r


def nav_redirect_chain(u):
    r = cli("fetch", f"{u}/redirect1", "-q", "--dump", "text")
    return "SECOND-PAGE-BODY" in r


def nav_meta_refresh(u):
    # Characterization, not endorsement: telemaco does NOT follow
    # `<meta http-equiv="refresh">`, it returns the source document. Pinned so
    # the behavior cannot change by accident. If someone implements the refresh,
    # this stage goes red on purpose and the expectation moves to the target.
    r = cli("fetch", f"{u}/meta-refresh.html", "-q", "--dump", "text")
    return "REDIRECTING" in r and "SECOND-PAGE-BODY" not in r


def nav_not_found(u):
    r = cli("fetch", f"{u}/gone", "--dump", "text")
    # A 404 must be reported, not silently rendered as an empty success.
    return r.rc != 0 or "404" in r.out or "404" in r.err


def nav_timeout(u):
    # The endpoint sleeps 8 seconds, so a ceiling loose enough to admit that
    # would pass even with the deadline ignored. Bound it just above the
    # requested 2 seconds instead, and require the failure to be reported.
    start = time.monotonic()
    try:
        r = cli("fetch", f"{u}/slow-response", "--timeout", "2", "--dump", "text", timeout=30)
    except subprocess.TimeoutExpired:
        return False
    elapsed = time.monotonic() - start
    return elapsed < 5 and r.rc != 0


def nav_wait_for_selector(u):
    r = cli("fetch", f"{u}/slow.html", "-q", "--selector", "#late", "--dump", "text")
    return "LATE-ELEMENT" in r


def state_cookies(u):
    r = cli("fetch", f"{u}/index.html", "-q", "--dump", "cookies")
    return "acc_cookie" in r


def state_storage_dir(u):
    d = tempfile.mkdtemp(prefix="acc-storage-")
    try:
        cli("fetch", f"{u}/index.html", "-q", "--storage-dir", d, "--dump", "text")
        r = cli("fetch", f"{u}/page2.html", "-q", "--storage-dir", d, "--dump", "cookies")
        return "acc_cookie" in r
    finally:
        shutil.rmtree(d, ignore_errors=True)


def state_local_storage(u):
    r = cli("fetch", f"{u}/index.html", "-q",
            "--eval", "(function(){ return localStorage.getItem('acc_key'); })()")
    return "acc_value" in r


def state_eval_iife(u):
    # A multi-statement --eval starting with const yields V8's empty completion
    # value, so the documented IIFE wrapper has to keep working.
    r = cli("fetch", f"{u}/index.html", "-q",
            "--eval", "(function(){ const t = document.title; return 'TITLE:' + t; })()")
    return "TITLE:Acceptance root" in r


def security_ssrf_gate(u):
    # The only stage that must run WITHOUT the gate opened.
    env = {k: v for k, v in os.environ.items() if k != "TELEMACO_ALLOW_PRIVATE_NETWORK"}
    r = cli("fetch", f"{u}/index.html", "--dump", "text", env=env)
    return "Acceptance root" not in r.out


def security_robots(u):
    r = cli("fetch", f"{u}/private/secret.html", "--obey-robots", "--dump", "text")
    return "SHOULD-NOT-BE-FETCHED" not in r.out


def security_private_flag(u):
    # With the flag the same fetch must succeed, or the gate is unusable.
    r = cli("fetch", f"{u}/index.html", "-q", "--allow-private-network", "--dump", "text")
    return "Acceptance root" in r


def render_screenshot(u):
    out = Path(tempfile.mkdtemp(prefix="acc-shot-")) / "shot.png"
    try:
        cli("fetch", f"{u}/layout.html", "-q", "--screenshot", str(out))
        return out.exists() and out.stat().st_size > 2000 and out.read_bytes()[:8] == b"\x89PNG\r\n\x1a\n"
    finally:
        shutil.rmtree(out.parent, ignore_errors=True)


def render_full_page(u):
    d = Path(tempfile.mkdtemp(prefix="acc-full-"))
    try:
        viewport, full = d / "v.png", d / "f.png"
        cli("fetch", f"{u}/layout.html", "-q", "--screenshot", str(viewport))
        env = {**BASE_ENV, "TELEMACO_SHOT_H": "4000"}
        cli("fetch", f"{u}/layout.html", "-q", "--screenshot", str(full), env=env)
        if not (viewport.exists() and full.exists()):
            return False
        # A taller viewport must capture more of a 1500px-tall page.
        return full.stat().st_size > viewport.stat().st_size
    finally:
        shutil.rmtree(d, ignore_errors=True)


def render_pdf(u):
    # Exercised through MCP because that is the surface an agent actually uses;
    # the tool returns the document as an embedded base64 resource.
    got = mcp_rpc([
        {"jsonrpc": "2.0", "id": 1, "method": "tools/call",
         "params": {"name": "browser_navigate", "arguments": {"url": f"{u}/layout.html"}}},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/call",
         "params": {"name": "browser_pdf", "arguments": {}}},
    ])
    import base64
    for msg in got:
        if msg.get("id") != 2:
            continue
        for item in msg.get("result", {}).get("content", []):
            blob = item.get("resource", {}).get("blob") or item.get("data")
            if not blob:
                continue
            try:
                raw = base64.b64decode(blob)
            except Exception:
                return False
            return raw.startswith(b"%PDF-") and len(raw) > 1000
    return False


def wait_for_port(port: int, timeout: float = 20) -> bool:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        with socket.socket() as s:
            s.settimeout(0.4)
            if s.connect_ex(("127.0.0.1", port)) == 0:
                return True
        time.sleep(0.15)
    return False


def cdp_serve(u):
    port = free_port()
    proc = subprocess.Popen(
        [BIN, "serve", "--port", str(port), "--allow-private-network"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=BASE_ENV)
    try:
        if not wait_for_port(port):
            return False
        import urllib.request
        with urllib.request.urlopen(f"http://127.0.0.1:{port}/json/version", timeout=10) as r:
            if r.status != 200:
                return False
            payload = json.loads(r.read())
        # The websocket endpoint is what a Puppeteer or Playwright client needs;
        # its absence makes the server useless even though HTTP answered.
        return payload.get("webSocketDebuggerUrl", "").startswith("ws://")
    except Exception:
        return False
    finally:
        proc.terminate()
        proc.wait(timeout=10)


def mcp_rpc(requests: list[dict], timeout: float = 90) -> list[dict]:
    payload = "".join(json.dumps(r) + "\n" for r in requests).encode()
    proc = subprocess.run([BIN, "mcp"], input=payload, capture_output=True,
                          timeout=timeout, env=BASE_ENV)
    out = []
    for line in proc.stdout.decode("utf-8", "replace").splitlines():
        line = line.strip()
        if line.startswith("{"):
            try:
                out.append(json.loads(line))
            except json.JSONDecodeError:
                pass
    return out


def mcp_tools_list(u):
    got = mcp_rpc([{"jsonrpc": "2.0", "id": 1, "method": "tools/list"}])
    if not got:
        return False
    tools = got[0].get("result", {}).get("tools", [])
    names = {t.get("name") for t in tools}
    return {"browser_navigate", "browser_markdown", "browser_click"} <= names


def mcp_navigate_and_read(u):
    got = mcp_rpc([
        {"jsonrpc": "2.0", "id": 1, "method": "tools/call",
         "params": {"name": "browser_navigate", "arguments": {"url": f"{u}/index.html"}}},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/call",
         "params": {"name": "browser_markdown", "arguments": {}}},
    ])
    for msg in got:
        if msg.get("id") == 2:
            text = "".join(c.get("text", "") for c in msg.get("result", {}).get("content", []))
            return "Acceptance root" in text and "INJECTED-BY-JS" in text
    return False


def mcp_fill_form(u):
    # The write half of the MCP surface: an agent that can only read is half a
    # browser. Values are read back through the page, not from the tool's reply.
    got = mcp_rpc([
        {"jsonrpc": "2.0", "id": 1, "method": "tools/call",
         "params": {"name": "browser_navigate", "arguments": {"url": f"{u}/forms.html"}}},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/call",
         "params": {"name": "browser_fill",
                    "arguments": {"selector": "#name", "value": "ACC-NAME"}}},
        {"jsonrpc": "2.0", "id": 3, "method": "tools/call",
         "params": {"name": "browser_select_option",
                    "arguments": {"selector": "#choice", "value": "b"}}},
        {"jsonrpc": "2.0", "id": 4, "method": "tools/call",
         "params": {"name": "browser_evaluate", "arguments": {"expression":
             "(function(){ return document.getElementById('name').value + '|' "
             "+ document.getElementById('choice').value; })()"}}},
    ])
    for msg in got:
        if msg.get("id") == 4:
            text = "".join(c.get("text", "") for c in msg.get("result", {}).get("content", []))
            return "ACC-NAME|b" in text
    return False


def speed_static_page(u):
    r = cli("fetch", f"{u}/page2.html", "-q", "--dump", "text")
    return "SECOND-PAGE-BODY" in r


def speed_scripted_page(u):
    r = cli("fetch", f"{u}/index.html", "-q", "--dump", "text")
    return "INJECTED-BY-JS" in r


def speed_parallel_scrape(u):
    r = cli("scrape", f"{u}/index.html", f"{u}/page2.html", f"{u}/layout.html",
            "--concurrency", "3", "--format", "json", "--quiet", timeout=120)
    return '"total_urls": 3' in r.out or '"total_urls":3' in r.out


STAGES: list[tuple[str, object]] = [
    ("dom.malformed-html", dom_malformed),
    ("dom.entities", dom_entities),
    ("dom.charset-latin1", dom_charset),
    ("dom.base-href", dom_base_href),
    ("dom.nested-tables", dom_nested_tables),
    ("js.dom-injection", js_dom_injection),
    ("js.timers", js_timers),
    ("js.fetch", js_fetch),
    ("js.xhr", js_xhr),
    ("js.async-await", js_async_await),
    ("js.custom-elements", js_custom_elements),
    ("js.dom-mutation", js_dom_mutation),
    ("extract.text", extract_text),
    ("extract.html", extract_html),
    ("extract.links", extract_links),
    ("extract.markdown", extract_markdown),
    ("extract.original-binary", extract_original_binary),
    ("extract.assets", extract_assets),
    ("nav.redirect-chain", nav_redirect_chain),
    ("nav.meta-refresh", nav_meta_refresh),
    ("nav.not-found", nav_not_found),
    ("nav.timeout", nav_timeout),
    ("nav.wait-for-selector", nav_wait_for_selector),
    ("state.cookies", state_cookies),
    ("state.storage-dir", state_storage_dir),
    ("state.local-storage", state_local_storage),
    ("state.eval-iife", state_eval_iife),
    ("security.ssrf-gate", security_ssrf_gate),
    ("security.robots", security_robots),
    ("security.private-network-flag", security_private_flag),
    ("render.screenshot", render_screenshot),
    ("render.viewport-height", render_full_page),
    ("render.pdf", render_pdf),
    ("protocol.cdp-serve", cdp_serve),
    ("protocol.mcp-tools-list", mcp_tools_list),
    ("protocol.mcp-navigate-read", mcp_navigate_and_read),
    ("protocol.mcp-fill-form", mcp_fill_form),
    ("speed.static-page", speed_static_page),
    ("speed.scripted-page", speed_scripted_page),
    ("speed.parallel-scrape", speed_parallel_scrape),
]


# -------------------------------------------------------------------- the runner


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--runs", type=int, default=1, help="timed runs per stage")
    parser.add_argument("--warmup", type=int, default=0, help="untimed runs first")
    parser.add_argument("--json", action="store_true", help="emit the CI contract")
    parser.add_argument("--filter", default="", help="only stages containing this")
    args = parser.parse_args()

    if not Path(BIN).exists():
        raise SystemExit(f"binary not found: {BIN}\nSet TELEMACO_BIN or build the release first.")

    stages = [s for s in STAGES if args.filter in s[0]]
    if not stages:
        raise SystemExit(f"no stage matches {args.filter!r}")

    results = []
    with Fixtures() as fx:
        for name, fn in stages:
            for _ in range(max(0, args.warmup)):
                try:
                    fn(fx.url)
                except Exception:
                    pass
            timings, ok = [], True
            for _ in range(max(1, args.runs)):
                start = time.monotonic()
                try:
                    passed = bool(fn(fx.url))
                except Exception as exc:
                    passed, _err = False, exc
                timings.append((time.monotonic() - start) * 1000.0)
                ok = ok and passed
            median_ms = statistics.median(timings)
            results.append({"name": name, "pass": ok, "median_ms": round(median_ms, 3)})
            if not args.json:
                mark = "\033[32mPASS\033[0m" if ok else "\033[31mFAIL\033[0m"
                print(f"  {mark}  {name:34s} {median_ms:8.1f} ms", flush=True)

    passed = sum(1 for r in results if r["pass"])
    if args.json:
        json.dump({"results": results}, sys.stdout, indent=2)
        sys.stdout.write("\n")
    else:
        total_ms = sum(r["median_ms"] for r in results)
        print(f"\n  {passed}/{len(results)} stages passed in {total_ms/1000:.1f}s of stage time")
        if passed != len(results):
            print("  failed: " + ", ".join(r["name"] for r in results if not r["pass"]))

    raise SystemExit(0 if passed == len(results) else 1)


if __name__ == "__main__":
    main()
