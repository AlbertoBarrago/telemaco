//! Line-ending handling for the line-level config editors.
//!
//! `str::lines()` drops the `\r` of a CRLF file, so every helper that rebuilds
//! a config from its lines silently converted it to LF. The file still parsed,
//! but the user's next `git diff` showed every line as changed.

/// The line ending a text file already uses.
pub fn line_ending(content: &str) -> &'static str {
    if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// Rewrites `updated` with the line ending `original` used.
pub fn with_line_ending(original: &str, updated: &str) -> String {
    if line_ending(original) == "\r\n" {
        normalize(updated).replace('\n', "\r\n")
    } else {
        updated.to_string()
    }
}

/// LF form of a text, for comparisons that must not care about line endings.
pub fn normalize(content: &str) -> String {
    content.replace("\r\n", "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crlf_survives_a_rewrite() {
        let original = "a: 1\r\nb: 2\r\n";
        // What a line-level editor produces after working in LF.
        let rebuilt = "a: 1\nb: 2\nc: 3\n";
        assert_eq!(with_line_ending(original, rebuilt), "a: 1\r\nb: 2\r\nc: 3\r\n");
        // An LF file stays LF.
        assert_eq!(with_line_ending("a: 1\n", rebuilt), rebuilt);
        // Already-CRLF input is not doubled.
        assert_eq!(with_line_ending(original, "x\r\ny\r\n"), "x\r\ny\r\n");
    }
}
