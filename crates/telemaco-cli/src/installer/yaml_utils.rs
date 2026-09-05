//! Line-level YAML editing for the targets that keep their config in YAML.
//!
//! Not a parser: these helpers splice text so the rest of the file, comments
//! and formatting included, survives byte for byte. Poolside and Hermes both
//! go through them, so a fix to the merge reaches both instead of one.


/// How a YAML key line carries its value.
pub enum KeyForm {
    /// `key:` with the value in the nested block below.
    Block,
    /// `key: {}` / `[]` / `null` / `~` - empty, so it can become a block.
    EmptyInline,
    /// `key: something` - a real inline value we must not guess at.
    Inline,
}

pub fn classify_key_line(line: &str, key: &str) -> Option<KeyForm> {
    let trimmed = line.trim_end();
    let rest = trimmed.trim_start().strip_prefix(key)?.strip_prefix(':')?;
    Some(match rest.trim() {
        "" => KeyForm::Block,
        "{}" | "[]" | "null" | "~" => KeyForm::EmptyInline,
        _ => KeyForm::Inline,
    })
}

/// Indent used by the children of the block opening at `key_idx`.
///
/// Deduced, not assumed: a file indented with four spaces would otherwise get
/// our two-space entry spliced in, which either nests it under the wrong key or
/// duplicates one.
pub fn child_indent(lines: &[String], key_idx: usize, key_indent: usize, end: usize) -> usize {
    lines[key_idx + 1..end]
        .iter()
        .find(|l| !l.trim().is_empty())
        .map(|l| indent_of(l))
        .filter(|i| *i > key_indent)
        .unwrap_or(key_indent + 2)
}

/// Shifts a relative YAML block to the given indent.
pub fn indent_block(item: &str, indent: usize) -> String {
    let pad = " ".repeat(indent);
    item.lines()
        .map(|l| {
            if l.trim().is_empty() {
                String::new()
            } else {
                format!("{}{}", pad, l)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Inserts `item` (written at indent zero) under the nested YAML keys in
/// `path`, creating only the levels that are missing and matching the
/// indentation the file already uses.
///
/// Appending a `hooks:` block blindly is what makes hand-built YAML go wrong:
/// a file that already has that key ends up with it twice, and so does the
/// `PreToolUse:` under it. This walks the keys that exist and splices into the
/// innermost one. It is a line-level merge, not a parser, so the rest of the
/// file (comments included) is left byte for byte.
///
/// `Err` when a key on the path holds an inline value (`hooks: {read: 1}`):
/// merging into that safely needs a real YAML parser, and writing a second
/// key with the same name would make the file invalid.
pub fn upsert_yaml_path(content: &str, path: &[&str], item: &str) -> Result<String, String> {
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let mut start = 0usize;
    let mut end = lines.len();
    let mut expected_indent = 0usize;

    for (depth, key) in path.iter().enumerate() {
        let found = (start..end).find(|&i| {
            indent_of(&lines[i]) == expected_indent && classify_key_line(&lines[i], key).is_some()
        });

        match found {
            Some(i) => {
                match classify_key_line(&lines[i], key).expect("checked above") {
                    KeyForm::Block => {}
                    // An empty inline value can simply become a block.
                    KeyForm::EmptyInline => {
                        lines[i] = format!("{}{}:", " ".repeat(expected_indent), key);
                    }
                    KeyForm::Inline => {
                        return Err(format!("'{}' holds an inline value; leaving it alone", key))
                    }
                }
                start = i + 1;
                let mut block_end = start;
                while block_end < end
                    && (lines[block_end].trim().is_empty()
                        || indent_of(&lines[block_end]) > expected_indent)
                {
                    block_end += 1;
                }
                end = block_end;
                expected_indent = child_indent(&lines, i, expected_indent, end);
            }
            None => {
                let mut block = String::new();
                if depth == 0 && !lines.is_empty() {
                    block.push('\n');
                }
                let mut indent = expected_indent;
                for missing in &path[depth..] {
                    block.push_str(&" ".repeat(indent));
                    block.push_str(missing);
                    block.push_str(":\n");
                    indent += 2;
                }
                block.push_str(&indent_block(item, indent));
                return Ok(splice_lines(&lines, end, &block));
            }
        }
    }

    Ok(splice_lines(&lines, start, &indent_block(item, expected_indent)))
}

/// The line where the block opened at `start` ends: the first line indented no
/// deeper than the one that opened it, trailing blank lines excluded.
pub fn block_extent(lines: &[&str], start: usize) -> usize {
    let indent = indent_of(lines[start]);
    let mut end = start + 1;
    while end < lines.len() && (lines[end].trim().is_empty() || indent_of(lines[end]) > indent) {
        end += 1;
    }
    while end > start + 1 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    end
}

/// Re-indents a block written at indent zero: its first line to `head`, its
/// children by the same shift, so nesting inside the block is preserved.
pub fn reindent_block(item: &str, head: usize, child: usize) -> String {
    let shift = child as isize - 2;
    item.lines()
        .enumerate()
        .map(|(i, l)| {
            if l.trim().is_empty() {
                return String::new();
            }
            let indent = if i == 0 {
                head
            } else {
                (indent_of(l) as isize + shift).max(0) as usize
            };
            format!("{}{}", " ".repeat(indent), l.trim_start())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Removes the first line matching `is_start` and every line nested under it.
///
/// Indentation alone is not a safe rule for "our block": the previous version
/// dropped any line starting with four spaces, which swallowed the *next* list
/// item and so deleted hooks belonging to the user. A block ends at the first
/// line indented no deeper than the line that opened it.
pub fn remove_yaml_block(content: &str, is_start: impl Fn(&str) -> bool) -> (String, bool) {
    let lines: Vec<&str> = content.lines().collect();
    let Some(start) = lines.iter().position(|l| is_start(l)) else {
        return (content.to_string(), false);
    };
    let indent = indent_of(lines[start]);
    let mut end = start + 1;
    while end < lines.len() && (lines[end].trim().is_empty() || indent_of(lines[end]) > indent) {
        end += 1;
    }
    // Blank lines at the tail separate the next block; leave them there.
    while end > start + 1 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }

    let mut out = String::new();
    for l in lines[..start].iter().chain(lines[end..].iter()) {
        out.push_str(l);
        out.push('\n');
    }
    (out, true)
}

/// Drops keys from `keys` that we may have created and that no longer have
/// anything under them, so removing our entries does not leave `mcp_servers:`
/// or `hooks:` dangling with a null value.
pub fn prune_empty_yaml_keys(content: &str, keys: &[&str]) -> String {
    let mut current = content.to_string();
    loop {
        let lines: Vec<&str> = current.lines().collect();
        let victim = lines.iter().enumerate().position(|(idx, l)| {
            let Some(name) = l.trim().strip_suffix(':') else { return false };
            if !keys.contains(&name) {
                return false;
            }
            let indent = indent_of(l);
            lines[idx + 1..]
                .iter()
                .find(|n| !n.trim().is_empty())
                .map_or(true, |n| indent_of(n) <= indent)
        });
        let Some(i) = victim else { break };
        let mut out = String::new();
        for l in lines[..i].iter().chain(lines[i + 1..].iter()) {
            out.push_str(l);
            out.push('\n');
        }
        current = out;
    }
    current
}

pub fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

pub fn splice_lines(lines: &[String], at: usize, block: &str) -> String {
    let mut out = String::new();
    for l in &lines[..at] {
        out.push_str(l);
        out.push('\n');
    }
    out.push_str(block);
    if !block.ends_with('\n') {
        out.push('\n');
    }
    for l in &lines[at..] {
        out.push_str(l);
        out.push('\n');
    }
    out
}

/// The child keys of our MCP entry that we write. Anything else under
/// `telemaco:` is the user's (an `env:`, a `timeout:`) and survives a rewrite.
const OWNED_ENTRY_KEYS: &[&str] = &["command", "args"];

/// An MCP server entry as YAML text, written at indent zero.
pub fn yaml_mcp_entry(name: &str, binary_path: &str, args: &[String]) -> String {
    format!(
        "{}:\n  command: \"{}\"\n  args:\n{}",
        name,
        binary_path,
        args.iter()
            .map(|a| format!("    - \"{}\"", a))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

/// Rewrites the `<name>:` entry to the one we would write today, keeping any
/// child key the user added to it.
///
/// Skipping an entry that already existed is how a reinstall stops following
/// the binary: once `telemaco` moves, the config keeps naming the old path and
/// fails silently.
pub fn refresh_yaml_entry(content: &str, name: &str, fresh_entry: &str) -> Option<String> {
    let head_line = format!("{}:", name);
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.iter().position(|l| l.trim() == head_line)?;
    let end = block_extent(&lines, start);
    let head = indent_of(lines[start]);
    let child = lines[start + 1..end]
        .iter()
        .find(|l| !l.trim().is_empty())
        .map(|l| indent_of(l))
        .filter(|i| *i > head)
        .unwrap_or(head + 2);

    let mut kept: Vec<String> = Vec::new();
    let mut keeping = false;
    for line in &lines[start + 1..end] {
        if !line.trim().is_empty() && indent_of(line) == child {
            let key = line.trim().split(':').next().unwrap_or("");
            keeping = !OWNED_ENTRY_KEYS.contains(&key);
        }
        if keeping {
            kept.push(line.to_string());
        }
    }

    let mut block: Vec<String> = reindent_block(fresh_entry, head, child)
        .lines()
        .map(|l| l.to_string())
        .collect();
    block.extend(kept);

    let mut out: Vec<String> = lines[..start].iter().map(|l| l.to_string()).collect();
    out.extend(block);
    out.extend(lines[end..].iter().map(|l| l.to_string()));
    Some(format!("{}\n", out.join("\n")))
}
