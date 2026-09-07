# telemaco-render

Optional scoped render layer for
[Telemaco](https://github.com/AlbertoBarrago/telemaco): selector cascade,
computed style, retained layout, and with the `paint` feature, text shaping and
raster output for screenshots and PDF.

## Place in the workspace

Telemaco's default build has no layout or paint engine, which is where its speed
and low memory come from. This crate adds one behind a feature flag: it builds a
layout tree from a `telemaco-dom` document plus its computed styles, lays it out
with `taffy`, and optionally rasterizes it with `tiny-skia`, `cosmic-text` and
`swash` (plus `resvg` for SVG and the `image` crate for bitmaps). It depends
only on `telemaco-dom`. The layer above is `telemaco-js`, which enables it
through its own `render` feature and feeds the geometry back to scripts, so
`getBoundingClientRect`, `elementFromPoint` and `IntersectionObserver` return
real values.

### Self-owned layout and text

The two libraries that define how a page renders, the layout engine and the
text engine, are Telemaco-owned forks published and maintained by this project:

- `taffy` here is the crate **`mentore`** (https://crates.io/crates/mentore),
  the Telemaco-maintained fork of the Taffy layout engine.
- `cosmic-text` here is the crate **`athena-text`**
  (https://crates.io/crates/athena-text), the Telemaco-maintained fork of the
  cosmic-text text engine, including the CSS line-breaking work.

Both publish exactly the version Telemaco builds against, under the same lib
names (`taffy` / `cosmic_text`), so the crate source is unchanged and the layout
and text output never move underneath a release. Anything that consumes `taffy`
or `cosmic-text` can switch to the forks with a one-line change in `Cargo.toml`.

## Features

| Feature | Effect |
|---------|--------|
| `paint` | Rasterization: `tiny-skia` pixmaps, `cosmic-text` and `swash` for shaping and glyph rendering, `ab_glyph` fallback, `image` for bitmap decoding, `resvg`/`usvg` for SVG, `ureq` for resource fetching, plus `base64`, `url` and `wuff`. Off by default; the layout core works without it. |

Without `paint` this is a layout-only crate.

## Usage

```rust
use telemaco_dom::parse_html;
use telemaco_render::layout_dom;

let tree = parse_html("<html><body><div style='width:100px;height:40px'></div></body></html>");
let layout = layout_dom(&tree, (1280.0, 800.0));

let div = tree.query_selector("div").unwrap().expect("no div");
println!("{:?}", layout.rects.get(&div));
```

With `paint`, `screenshot_png(&tree, (1280.0, 800.0), Some(base_url))` returns
PNG bytes; the `_scrolled` and `_at_animation_time` variants pin scroll offset
and animation time for deterministic captures.

## Invariants

- **The color emoji font is excluded from the published package.** The ~10 MB
  `assets/noto-color-emoji.ttf` would put the crate over the crates.io size
  limit on its own, so `build.rs` detects its absence and a registry build falls
  back to monochrome emoji. The git tree and the release binaries keep the color
  face.
- **Rendering is deterministic and host-independent.** Only the embedded fonts
  are used, with no system-font scan, and paint is CPU-only with no system
  graphics dependency. Never add hostname-specific layout, style or resource
  behavior.
- `cosmic-text` and `swash` are pinned to exact versions that share one
  canonical variation model. Do not bump them independently.

Part of [Telemaco](https://github.com/AlbertoBarrago/telemaco).
