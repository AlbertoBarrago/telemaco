//! Keyword-based content narrowing for markdown output.
//!
//! Splits a markdown document into blocks (paragraphs, headings, list
//! items, code fences) and keeps only the blocks that contain at least
//! one of the focus keywords, plus a window of surrounding blocks and
//! the heading chain above each hit. Document title is always kept.

/// Result of applying a focus filter.
pub struct FocusOutcome {
    /// Filtered markdown (empty when nothing matched).
    pub text: String,
    /// Number of blocks kept (including context and headings).
    pub kept: usize,
    /// Total number of blocks in the input.
    pub total: usize,
    /// Whether any block directly matched a keyword.
    pub matched: bool,
}

/// One markdown block: a run of consecutive non-blank lines, with blank
/// lines inside fenced code blocks kept internal to the block.
struct Block {
    lines: Vec<String>,
    start: usize,
    /// Index of the heading this block starts with, if any.
    heading_level: Option<usize>,
}

/// Split markdown into blocks, respecting fenced code blocks so that
/// blank lines inside a fence do not split it.
fn split_blocks(markdown: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut start = 0usize;
    let mut in_fence = false;

    for (i, line) in markdown.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            if !current.is_empty() {
                current.push(line.to_string());
            } else {
                start = i;
                current.push(line.to_string());
            }
            in_fence = !in_fence;
            continue;
        }
        if line.trim().is_empty() && !in_fence {
            if !current.is_empty() {
                blocks.push(make_block(current, start));
                current = Vec::new();
            }
            continue;
        }
        if current.is_empty() {
            start = i;
        }
        current.push(line.to_string());
    }
    if !current.is_empty() {
        blocks.push(make_block(current, start));
    }
    blocks
}

fn make_block(lines: Vec<String>, start: usize) -> Block {
    let heading_level = lines
        .first()
        .and_then(|l| {
            let t = l.trim_start();
            let hashes = t.bytes().take_while(|&b| b == b'#').count();
            if (1..=6).contains(&hashes) && t.as_bytes().get(hashes) == Some(&b' ') {
                Some(hashes)
            } else {
                None
            }
        });
    Block {
        lines,
        start,
        heading_level,
    }
}

impl Block {
    fn text(&self) -> String {
        self.lines.join("\n")
    }
}

/// Case-insensitive substring match for one block.
fn block_matches(block: &Block, keywords: &[String]) -> bool {
    let text = block.text().to_lowercase();
    keywords.iter().any(|kw| text.contains(&kw.to_lowercase()))
}

/// Apply the focus filter.
///
/// `keywords` empty means passthrough: the whole document is returned.
pub fn focus_filter(markdown: &str, keywords: &[String], context: usize) -> FocusOutcome {
    if keywords.is_empty() {
        let total = split_blocks(markdown).len();
        return FocusOutcome {
            text: markdown.to_string(),
            kept: total,
            total,
            matched: true,
        };
    }

    let blocks = split_blocks(markdown);
    let total = blocks.len();
    if total == 0 {
        return FocusOutcome {
            text: String::new(),
            kept: 0,
            total: 0,
            matched: false,
        };
    }

    // Direct hits.
    let mut keep: Vec<bool> = vec![false; total];
    let mut matched = false;
    for (i, b) in blocks.iter().enumerate() {
        if block_matches(b, keywords) {
            keep[i] = true;
            matched = true;
        }
    }

    // Context window: expand each hit by `context` blocks on each side.
    if context > 0 {
        let hits: Vec<usize> = (0..total).filter(|&i| keep[i]).collect();
        for &h in &hits {
            for i in h.saturating_sub(context)..=(h + context).min(total - 1) {
                keep[i] = true;
            }
        }
    }

    // Heading chain: for every kept block, keep the currently-open
    // headings above it (all levels shallower than the closest heading
    // that intervenes).
    let mut open: Vec<usize> = Vec::new(); // indices of open headings
    for i in 0..total {
        if let Some(level) = blocks[i].heading_level {
            open.retain(|&h| blocks[h].heading_level.unwrap() < level);
            open.push(i);
        }
        if keep[i] {
            for &h in &open {
                keep[h] = true;
            }
        }
    }

    // Document title: first H1 always kept (only when something matched,
    // so a zero-match filter returns a genuinely empty result).
    if matched {
        if let Some(first_h1) = blocks.iter().position(|b| b.heading_level == Some(1)) {
            keep[first_h1] = true;
        }
    }

    // Reassemble.
    let kept_count = keep.iter().filter(|&&k| k).count();
    let mut out = String::new();
    for (i, b) in blocks.iter().enumerate() {
        if !keep[i] {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&b.text());
        out.push('\n');
    }

    FocusOutcome {
        text: out,
        kept: kept_count,
        total,
        matched,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "\
# Connect API Guide

Intro paragraph about the API.

## Rate limits

Connect REST API has a per-user rate limit. Exceeding it returns a 503.

More detail on 503 handling.

## Authentication

Authenticate with OAuth 2.0 bearer tokens.

Sidebar noise paragraph with no keywords at all.

## Examples

```python
client.feeds.list()
```

Closing paragraph about rate limit retries.
";

    #[test]
    fn empty_keywords_passthrough() {
        let out = focus_filter(DOC, &[], 1);
        assert_eq!(out.text, DOC);
        assert_eq!(out.kept, out.total);
        assert!(out.matched);
    }

    #[test]
    fn keeps_hit_and_context() {
        let kw = vec!["503".to_string()];
        let out = focus_filter(DOC, &kw, 1);
        // Title + "## Rate limits" heading + hit + following block.
        assert!(out.text.contains("# Connect API Guide"));
        assert!(out.text.contains("## Rate limits"));
        assert!(out.text.contains("returns a 503"));
        assert!(out.text.contains("More detail on 503 handling."));
        // Two blocks away must be dropped.
        assert!(!out.text.contains("Authenticate with OAuth"));
        assert!(out.matched);
        assert!(out.kept < out.total);
    }

    #[test]
    fn context_zero_keeps_only_hit_and_heading() {
        // "returns a 503" matches only the first block; the next block
        // mentions 503 but not the phrase.
        let kw = vec!["returns a 503".to_string()];
        let out = focus_filter(DOC, &kw, 0);
        assert!(out.text.contains("returns a 503"));
        assert!(!out.text.contains("More detail on 503 handling."));
        assert!(!out.text.contains("Closing paragraph"));
    }

    #[test]
    fn case_insensitive_match() {
        let kw = vec!["OAUTH".to_string()];
        let out = focus_filter(DOC, &kw, 0);
        assert!(out.text.contains("Authenticate with OAuth"));
    }

    #[test]
    fn keyword_in_code_block_matches() {
        let kw = vec!["feeds.list".to_string()];
        let out = focus_filter(DOC, &kw, 0);
        assert!(out.text.contains("client.feeds.list()"));
        assert!(out.text.contains("## Examples"));
    }

    #[test]
    fn fence_survives_internal_blank_lines() {
        let doc = "## Examples\n\n```python\nx = 1\n\ny = 2\n\n```\n\nUnrelated tail.\n";
        let kw = vec!["y = 2".to_string()];
        let out = focus_filter(doc, &kw, 0);
        assert!(out.text.contains("x = 1"));
        assert!(out.text.contains("y = 2"));
        assert!(out.text.ends_with("```\n"));
        assert!(!out.text.contains("Unrelated tail."));
    }

    #[test]
    fn no_match_reports_explicitly() {
        let kw = vec!["zzz_nonexistent".to_string()];
        let out = focus_filter(DOC, &kw, 1);
        assert!(!out.matched);
        assert_eq!(out.text, "");
        assert_eq!(out.kept, 0);
    }

    #[test]
    fn heading_chain_kept_across_sections() {
        let doc = "# Top\n\n## A\n\nalpha text\n\n### Deep\n\nneedle keyword here\n\nother text\n";
        let kw = vec!["needle".to_string()];
        let out = focus_filter(doc, &kw, 0);
        assert!(out.text.contains("# Top"));
        assert!(out.text.contains("## A"));
        assert!(out.text.contains("### Deep"));
        assert!(out.text.contains("needle keyword here"));
        assert!(!out.text.contains("alpha text"));
        assert!(!out.text.contains("other text"));
    }

    #[test]
    fn overlapping_windows_dedup() {
        let doc = "k1 hit\n\nfiller\n\nk2 hit\n\nfiller\n\nk3 hit\n\nfiller\n";
        let kw = vec!["k1".to_string(), "k3".to_string()];
        // context=1: windows around k1 (0-1) and k3 (3-5) leave k2 out.
        let out = focus_filter(doc, &kw, 1);
        assert!(out.text.contains("k1 hit"));
        assert!(!out.text.contains("k2 hit"));
        assert!(out.text.contains("k3 hit"));
        assert_eq!(out.kept, 5);
        assert_eq!(out.total, 6);
        // context=2: windows overlap, everything survives.
        let out = focus_filter(doc, &kw, 2);
        assert!(out.text.contains("k2 hit"));
        assert_eq!(out.kept, 6);
    }
}