`--dump markdown` converts the rendered page to markdown.

```bash
telemaco fetch https://example.com --dump markdown
```

## What gets converted

- Headings (`<h1>` through `<h6>`)
- Paragraphs, line breaks
- Bold, italic, code spans
- Links (with `href`)
- Images (with `src` and `alt`)
- Ordered and unordered lists
- Block quotes
- Code blocks (`<pre>`, `<code>`)
- Tables

## What gets stripped

- `<script>`, `<style>`, `<noscript>`
- Inline styles
- ARIA attributes
- Tracking pixels and beacons

## Save to file

```bash
telemaco fetch https://docs.example.com/page --dump markdown -o page.md
```

## For RAG / LLM context

```bash
telemaco fetch https://docs.example.com/page --dump markdown --quiet
```

`--quiet` strips info logging so the output is just markdown.

## Wait for SPA content

For pages that render content client-side:

```bash
telemaco fetch https://my-spa.example --wait-until load --dump markdown
```

## Narrow to a region

`--selector` restricts the conversion to a CSS selector:

```bash
telemaco fetch https://example.com --selector "main" --dump markdown
telemaco fetch https://example.com --selector "article.post" --dump markdown
```

Useful for skipping nav, sidebars, and footers.

## Narrow by keyword (`--focus`)

`--focus` filters the markdown output down to the blocks that contain at
least one of the keywords (case-insensitive, repeatable). Each hit keeps a
window of surrounding blocks (`--focus-context`, default 1), the heading
chain above it, and the page title. A summary (`focus: kept N of M blocks`)
is printed to stderr, so stdout stays pipeable with `--quiet`.

```bash
telemaco fetch https://example.com/docs/page --dump markdown --quiet \
  --focus "rate limit" --focus 503
```

If no block matches, the command prints `focus: no blocks matched [...]`
to stderr and emits an empty document — loosen the keywords or raise the
context window.

Measured on a rendered page (release build, `--focus-context 1`):

| Page | Full markdown | Focused | Reduction |
|---|---|---|---|
| docs.rs/serde (static) | 9610 B | 2198 B | −77% |
