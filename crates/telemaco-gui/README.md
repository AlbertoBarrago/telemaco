# telemaco-gui

Native interactive window over the Telemaco engine. One binary: an egui window
whose page surface is the Telemaco renderer itself, driven in-process through
the same CDP layer Puppeteer and Playwright use. The engine crate is embedded
unchanged (the `CdpContext` + `dispatch` pattern, like telemaco-mcp).

## Run

```bash
cargo build --release -p telemaco-gui
./target/release/telemaco-gui https://example.com
```

Flags: `--proxy`, `--stealth`, `--user-agent`, `--allow-private-network`
(required for `http://localhost` URLs, same SSRF gate as the CLI).

## What works

- Omnibar (Cmd+L focuses it), Back/Forward/Reload, live adaptive frame
  streaming (120 ms while active, 450 ms idle)
- Trusted input: mouse press/release with click counting, wheel scrolling with
  the engine's nested scroll chaining, text insertion, Enter/Backspace/arrow
  keys (same trusted-event snippets the CDP Input domain generates)
- Window resize tracks the page viewport via device metrics emulation
- Page title in the window title bar; load spinner; navigation errors surfaced
  in a status line

## Not yet (v2 candidates)

- Multiple tabs, page-selection copy (Cmd+C), downloads, JS dialog surfacing,
  HiDPI device-scale refinement beyond the default screen scale.

## Architecture

- `src/engine.rs` - background thread owning `telemaco_cdp::CdpContext`; drives
  `Page.navigate`, `Page.reload`, `Page.getNavigationHistory`,
  `Page.navigateToHistoryEntry`, `Page.captureScreenshot`, `Input.dispatch*`,
  `Emulation.setDeviceMetricsOverride`, and drains `pending_events` for the
  load lifecycle. Frames are captured as PNG, decoded to RGBA, and paced
  adaptively using the JS runtime's activity generation as a damage signal.
- `src/app.rs` - egui toolbar + image widget; maps pointer, wheel, and key
  events to engine commands. Coordinates are CSS pixels (device scale factor
  follows `pixels_per_point`, so frames land crisp on retina displays).

## macOS .dmg packaging

```bash
scripts/make-dmg.sh    # builds release and packs target/dmg/Telemaco.dmg (unsigned)
```

An unsigned app opens with right-click > Open the first time. Signing and
notarization need an Apple Developer identity:

```bash
codesign --deep --force --options runtime \
  --sign "Developer ID Application: NAME (TEAMID)" target/dmg/Telemaco.app
xcrun notarytool submit target/dmg/Telemaco.dmg --apple-id ... --team-id ... --password ... --wait
```

## Tests

```bash
cargo nextest run --release -p telemaco-gui
```

`engine_smoke` runs the real V8 + render pipeline against a loopback fixture
and asserts a navigated status plus a nonblank frame.