use std::path::Path;

/// The Noto Color Emoji face is ~10 MB of embedded bitmaps, which pushes the
/// crates.io package past the 10 MiB upload limit on its own. It is excluded
/// from the published package (see `exclude` in Cargo.toml) and stays in the
/// git tree, so a source checkout and the release binaries keep color emoji
/// while a registry build degrades to the monochrome fallback.
///
/// Detect the file here rather than behind a cargo feature: a feature would
/// let a registry consumer enable something whose bytes are not there and hit
/// a missing-file error from `include_bytes!`.
fn main() {
    let font = Path::new("assets/noto-color-emoji.ttf");
    println!("cargo::rerun-if-changed=assets/noto-color-emoji.ttf");
    println!("cargo::rustc-check-cfg=cfg(telemaco_emoji_font)");
    if font.exists() {
        println!("cargo::rustc-cfg=telemaco_emoji_font");
    }
}
