The recipe for the most common extension: a new JS Web API.

Worked example: `crypto.subtle.digest`, real implementation backed by a Rust hash op.

## 1. Add the Rust op

In `crates/telemaco-js/src/ops.rs`:

```rust
#[op2]
#[buffer]
fn op_subtle_digest(#[string] algorithm: &str, #[buffer] data: &[u8]) -> Vec<u8> {
    use sha1::Digest as _;
    match algorithm.to_ascii_uppercase().as_str() {
        "SHA-1"   => sha1::Sha1::digest(data).to_vec(),
        "SHA-256" => sha2::Sha256::digest(data).to_vec(),
        "SHA-384" => sha2::Sha384::digest(data).to_vec(),
        "SHA-512" => sha2::Sha512::digest(data).to_vec(),
        _         => sha2::Sha256::digest(data).to_vec(),
    }
}
```

## 2. Register the op

In the same file, `build_extension()`:

```rust
ops: std::borrow::Cow::Owned(vec![
    op_dom(),
    op_console_msg(),
    // ...
    op_subtle_digest(),
]),
```

## 3. Add the JS shim

In `crates/telemaco-js/js/bootstrap.js`:

```js
globalThis.crypto = globalThis.crypto || {};
globalThis.crypto.subtle = globalThis.crypto.subtle || {};
globalThis.crypto.subtle.digest = function digest(algorithm, data) {
  const algName = typeof algorithm === 'string' ? algorithm : algorithm.name;
  const bytes = data instanceof ArrayBuffer
    ? new Uint8Array(data)
    : new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  const out = Deno.core.ops.op_subtle_digest(algName, bytes);
  return Promise.resolve(out.buffer);
};
```

## 4. Add a dependency if needed

`crates/telemaco-js/Cargo.toml`:

```toml
sha1 = "0.10"
sha2 = "0.10"
```

## 5. Smoke test

```bash
cargo build --release --features render
./target/release/telemaco fetch https://example.com --eval "
  crypto.subtle.digest('SHA-256', new TextEncoder().encode('hi'))
    .then(buf => Array.from(new Uint8Array(buf)).map(b => b.toString(16).padStart(2, '0')).join(''))
"
```

## Tips

- Keep the JS shim thin. All side effects go through ops.
- Use `Promise.resolve` to keep async-shaped APIs callable from sync ops.
- Match the spec: page scripts feature-detect Web APIs by name and shape.
- DOM mutations go through `op_dom`, not new ops.
- For events that need to fire across handlers, use the existing `_makeListenerBox` helper in `bootstrap.js`.

## Worked examples in the tree

- Web API with op + JS shim: `crypto.subtle.digest` (above).
- Web API in pure JS (no op): `DOMParser` in `bootstrap.js`.
- Web API with async event firing: `WebSocket`, `IntersectionObserver` in `bootstrap.js`.
