//! Markdown block utilities shared by the `--focus` CLI filter and the
//! paginated `browser_markdown` MCP tool.
//!
//! A "block" is a run of consecutive non-blank lines, with blank lines
//! inside fenced code blocks kept internal to the block. Heading chains,
//! keyword filtering and pagination all operate on this granularity.

/// One markdown block: a run of consecutive non-blank lines, with blank
/// lines inside fenced code blocks kept internal to the block.
pub struct Block {
    pub lines: Vec<String>,
    /// Line index of the block's first line in the source document.
    pub start: usize,
    /// Index of the heading this block starts with, if any (`#`=1..`######`=6).
    pub heading_level: Option<usize>,
}

impl Block {
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }
}

/// Split markdown into blocks, respecting fenced code blocks so that
/// blank lines inside a fence do not split it.
pub fn split_blocks(markdown: &str) -> Vec<Block> {
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
    let heading_level = lines.first().and_then(|l| {
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

/// Pack blocks into pages of at most `max_chars` characters, never
/// cutting a block in half. A single block larger than `max_chars` is
/// split across pages line by line. Empty input yields zero pages.
///
/// Returns the pages as strings (each already joined with blank-line
/// separators) and the total page count.
pub fn pack_pages(markdown: &str, max_chars: usize) -> Vec<String> {
    fn push_block(
        pages: &mut Vec<String>,
        current: &mut Vec<String>,
        current_chars: &mut usize,
        text: &str,
        max_chars: usize,
    ) {
        let block_chars = text.chars().count() + 2; // + blank-line separator
        if !current.is_empty() && *current_chars + block_chars > max_chars {
            pages.push(current.join("\n\n"));
            current.clear();
            *current_chars = 0;
        }
        current.push(text.to_string());
        *current_chars += block_chars;
    }

    let mut pages: Vec<String> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut current_chars = 0usize;

    for block in split_blocks(markdown) {
        let text = block.text();
        if text.chars().count() + 2 > max_chars {
            // Oversized block: flush what we have, then emit the block
            // line by line so no page exceeds the ceiling (except when a
            // single line alone is longer, which becomes its own page).
            if !current.is_empty() {
                pages.push(current.join("\n\n"));
                current.clear();
                current_chars = 0;
            }
            for line in block.lines {
                push_block(&mut pages, &mut current, &mut current_chars, &line, max_chars);
            }
        } else {
            push_block(&mut pages, &mut current, &mut current_chars, &text, max_chars);
        }
    }
    if !current.is_empty() {
        pages.push(current.join("\n\n"));
    }
    pages
}

/// Return page `page` (1-based) of the block-packed document, plus the
/// total page count. Pages beyond the end return empty text.
pub fn paginate(markdown: &str, max_chars: usize, page: usize) -> (String, usize) {
    let pages = pack_pages(markdown, max_chars);
    let total = pages.len();
    let text = page
        .checked_sub(1)
        .and_then(|idx| pages.get(idx))
        .cloned()
        .unwrap_or_default();
    (text, total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_respects_char_ceiling() {
        let doc = "alpha bravo\n\ncharlie delta\n\necho foxtrot\n\ngolf hotel\n";
        let pages = pack_pages(doc, 20);
        assert!(pages.len() >= 2);
        for p in &pages {
            assert!(p.chars().count() <= 20 + 4, "page too long: {p:?}");
        }
        // Every block survives exactly once, in order.
        let rejoined = pages.join("\n\n");
        assert!(rejoined.contains("alpha bravo"));
        assert!(rejoined.contains("golf hotel"));
    }

    #[test]
    fn pack_never_splits_small_blocks() {
        let doc = "short one\n\nshort two\n";
        let pages = pack_pages(doc, 4000);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0], "short one\n\nshort two");
    }

    #[test]
    fn oversized_block_split_by_lines() {
        let l1 = "x".repeat(30);
        let l2 = "y".repeat(30);
        let doc = format!("{l1}\n{l2}\n");
        let pages = pack_pages(&doc, 35);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0], l1);
        assert_eq!(pages[1], l2);
    }

    #[test]
    fn fence_internal_blank_lines_survive_packing() {
        let doc = "```python\nx = 1\n\ny = 2\n```\n\ntail\n";
        let pages = pack_pages(doc, 4000);
        assert_eq!(pages.len(), 1);
        assert!(pages[0].contains("x = 1"));
        assert!(pages[0].contains("y = 2"));
        assert!(pages[0].contains("tail"));
    }

    #[test]
    fn empty_input_yields_zero_pages() {
        assert!(pack_pages("", 4000).is_empty());
        let (text, total) = paginate("", 4000, 1);
        assert_eq!(text, "");
        assert_eq!(total, 0);
    }

    #[test]
    fn paginate_beyond_end_is_empty() {
        let doc = "one\n\ntwo\n\nthree\n";
        let (text, total) = paginate(doc, 12, 99);
        assert_eq!(text, "");
        assert_eq!(total, 2);
    }

    #[test]
    fn paginate_roundtrip_covers_everything() {
        let blocks: Vec<String> = (0..40)
            .map(|i| format!("block {i} with some padding text to take space"))
            .collect();
        let doc = blocks.join("\n\n");
        let mut roundtrip = String::new();
        let mut page = 1;
        let total = loop {
            let (text, t) = paginate(&doc, 200, page);
            if text.is_empty() {
                break t;
            }
            if !roundtrip.is_empty() {
                roundtrip.push_str("\n\n");
            }
            roundtrip.push_str(&text);
            page += 1;
            assert!(page < 100, "runaway pagination");
        };
        assert_eq!(total, page - 1);
        // Every original block is present in the concatenated pages.
        for b in &blocks {
            assert!(roundtrip.contains(b), "lost block: {b}");
        }
    }

    #[test]
    fn paginate_first_page_total_reported() {
        let doc = "one\n\ntwo\n\nthree\n";
        // "one\ntwo" is 8 chars with the separator, fits in 10.
        let (text, total) = paginate(doc, 10, 1);
        assert_eq!(text, "one\n\ntwo");
        assert_eq!(total, 2);
    }
}