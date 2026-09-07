# telemaco-dom

DOM tree and CSS selector matching for the
[Telemaco](https://github.com/AlbertoBarrago/telemaco) headless browser engine.

## Place in the workspace

This is the bottom layer of the Telemaco workspace. It owns the parsed document
(`DomTree`, `Node`, `NodeId`, shadow roots) and the selector engine, built on
`html5ever` and Servo's `selectors`. It depends on no other Telemaco crate.
Everything above it (`telemaco-js` for the JS bindings, `telemaco-render` for
layout and paint, `telemaco-browser` for pages) reads and mutates the tree
through this API. Cross-crate calls go through the layer above, never sideways.

## Usage

```rust
use telemaco_dom::parse_html;

let tree = parse_html("<html><body><a href='/next'>Next</a></body></html>");

let link = tree.query_selector("a").unwrap().expect("no anchor");
println!("{}", tree.text_content(link));
let href = tree.with_node(link, |n| n.get_attribute("href").map(str::to_string));
println!("{:?}", href.flatten());
```

`parse_fragment` and `parse_fragment_with_context` parse fragments;
`query_selector_all`, `query_selector_from` and `matches_selector` cover the
rest of the query surface.

## Invariants

- The reparenting guards in `src/tree.rs` are load-bearing. `append_child` and
  `insert_before` reject cycles (inserting an ancestor of the target is a
  no-op), and `descendants()` keeps a length cap. A cyclic reparent used to make
  traversal loop forever and hang the engine, uninterruptible by the runtime
  watchdog. Do not remove either guard.
- The tree is not `Send`. It lives on the same thread as the V8 isolate that
  drives it.

## Features

This crate has no cargo features.

Part of [Telemaco](https://github.com/AlbertoBarrago/telemaco).
