Interception lets you inspect, block, or modify every request a page makes, including JavaScript `fetch()` and XHR.

## Block trackers with stealth

`--stealth` ships with a tracker blocklist (known analytics and fingerprinting endpoints) that applies to every page without per-script setup. See [Configure stealth and proxies](Configure-stealth-and-proxies.md).

## From the Rust library

If you embed the engine with the `telemaco` crate, interception is a native API on `Page`: `on_request` / `on_response` callbacks, an `enable_interception()` channel that can block, mock, or rewrite requests, and `add_preload_script` to run code before the page's own scripts. See [Use as a Rust library](Use-as-a-Rust-library.md#intercept-requests).
