use std::fs;
use std::path::Path;

use crate::installer::text_utils::{normalize, with_line_ending};

pub const TELEMACO_START_MARKER: &str = "<!-- TELEMACO_START -->";
pub const TELEMACO_END_MARKER: &str = "<!-- TELEMACO_END -->";

/// The block written into an agent's instructions file.
///
/// Kept deliberately short: it sits in the agent's context on every single
/// turn. The MCP server repeats the same directive in its `initialize`
/// response, and the prompt hook injects a fuller version only on prompts that
/// actually look web-bound, so this copy only has to carry what an agent needs
/// before it has touched either.
pub fn get_instructions_block(stealth_default: bool) -> String {
    let stealth_flag = if stealth_default { " --stealth" } else { "" };
    format!(
r#"{TELEMACO_START_MARKER}
## Web access: use Telemaco

For any web work (visiting a URL, reading online docs, inspecting or scraping a
page, searching), use Telemaco instead of built-in web search or `curl`/`wget`.
Telemaco runs a real browser: V8, a full DOM, and stealth against bot detection,
so it sees pages that a plain fetch cannot.

MCP tools, when connected: `browser_navigate` (open a URL), `browser_markdown`
(page as clean Markdown), `browser_snapshot` (DOM and interactive elements),
`browser_extract` (CSS selectors), `browser_search` (find in page),
`browser_click` / `browser_fill` / `browser_type` (forms).

CLI, when they are not:
- `telemaco{stealth_flag} fetch <url> --dump markdown` (also `text`, `html`)
- `telemaco{stealth_flag} fetch <url> --screenshot <path.png>` (needs a build
  with the `render` feature; without it the command says so and exits)

Rules:
1. To search, navigate to `https://duckduckgo.com/html/?q=<query>` and read the
   results with `browser_markdown`, then open the target page. Never fall back
   to built-in search, `curl` or `wget`.
2. Loopback and RFC1918 are blocked by default (SSRF guard). Pass
   `--allow-private-network` only when deliberately testing a local URL.
3. State the URL before visiting it (`Navigating to: <url>`) and cite every
   source URL you consulted.
{TELEMACO_END_MARKER}"#
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Created,
    Updated,
    Unchanged,
    Removed,
    NotFound,
}

/// What `replace_or_append_marked_section` would do, without doing it.
pub fn plan_marked_section(file_path: &Path, block: &str) -> std::io::Result<Action> {
    if !file_path.exists() {
        return Ok(Action::Created);
    }
    let content = fs::read_to_string(file_path)?;
    match locate_marked_section(&content) {
        MarkedSection::Malformed(why) => Err(malformed(why)),
        // Compared in LF: a CRLF file holds the same block with `\r\n`, and
        // calling that a change rewrote the file on every single install.
        MarkedSection::One(s, e) if normalize(&content[s..e]) == block => Ok(Action::Unchanged),
        _ => Ok(Action::Updated),
    }
}

pub fn replace_or_append_marked_section(file_path: &Path, block: &str) -> std::io::Result<Action> {
    if !file_path.exists() {
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = format!("{}\n", block);
        crate::installer::json_utils::atomic_write_file(file_path, &content)?;
        return Ok(Action::Created);
    }

    let content = fs::read_to_string(file_path)?;

    match locate_marked_section(&content) {
        MarkedSection::Malformed(why) => return Err(malformed(why)),
        MarkedSection::One(s, e) => {
            if normalize(&content[s..e]) == block {
                return Ok(Action::Unchanged);
            }
            let new_content = format!("{}{}{}", &content[..s], block, &content[e..]);
            let new_content = with_line_ending(&content, &new_content);
            crate::installer::json_utils::atomic_write_file(file_path, &new_content)?;
            return Ok(Action::Updated);
        }
        MarkedSection::Absent => {}
    }

    // Append to existing content
    let trimmed = content.trim_end();
    let sep = if trimmed.is_empty() { "" } else { "\n\n" };
    let new_content = format!("{}{}{}\n", trimmed, sep, block);
    let new_content = with_line_ending(&content, &new_content);
    crate::installer::json_utils::atomic_write_file(file_path, &new_content)?;
    Ok(Action::Updated)
}

/// What `remove_marked_section` would do, without doing it.
pub fn plan_remove_marked_section(file_path: &Path) -> std::io::Result<Action> {
    if !file_path.exists() {
        return Ok(Action::NotFound);
    }
    let content = fs::read_to_string(file_path)?;
    match locate_marked_section(&content) {
        MarkedSection::Malformed(why) => Err(malformed(why)),
        MarkedSection::One(_, _) => Ok(Action::Removed),
        MarkedSection::Absent => Ok(Action::NotFound),
    }
}

pub fn remove_marked_section(file_path: &Path) -> std::io::Result<Action> {
    if !file_path.exists() {
        return Ok(Action::NotFound);
    }

    let content = fs::read_to_string(file_path)?;

    let (s, full_end) = match locate_marked_section(&content) {
        MarkedSection::Malformed(why) => return Err(malformed(why)),
        MarkedSection::Absent => return Ok(Action::NotFound),
        MarkedSection::One(s, e) => (s, e),
    };

    let before = content[..s].trim_end();
    let after = content[full_end..].trim_start();
    let joined = if before.is_empty() {
        after.to_string()
    } else if after.is_empty() {
        before.to_string()
    } else {
        format!("{}\n\n{}", before, after)
    };

    let symlinked = fs::symlink_metadata(file_path)
        .map_or(false, |m| m.file_type().is_symlink());
    if joined.trim().is_empty() && !symlinked {
        // Nothing of the user's left: the file was ours to begin with. A
        // symlink is the user's dotfiles setup, so it is emptied instead.
        if let Err(e) = fs::remove_file(file_path) {
            return Err(e);
        }
    } else {
        let new_content = format!("{}\n", joined.trim());
        let new_content = with_line_ending(&content, &new_content);
        crate::installer::json_utils::atomic_write_file(file_path, &new_content)?;
    }
    Ok(Action::Removed)
}

/// What the Telemaco markers in a file look like.
enum MarkedSection {
    /// No markers: the block gets appended.
    Absent,
    /// Exactly one well-formed block, as a byte range.
    One(usize, usize),
    /// Anything else. Guessing here is how the user's text gets eaten: a stray
    /// start marker with no end used to pair up with the end of a block
    /// appended later, and everything in between was replaced.
    Malformed(String),
}

fn locate_marked_section(content: &str) -> MarkedSection {
    let starts = content.matches(TELEMACO_START_MARKER).count();
    let ends = content.matches(TELEMACO_END_MARKER).count();

    match (starts, ends) {
        (0, 0) => MarkedSection::Absent,
        (1, 1) => {
            let start = content.find(TELEMACO_START_MARKER).expect("counted one");
            let end = content.find(TELEMACO_END_MARKER).expect("counted one");
            if end < start {
                MarkedSection::Malformed(
                    "the Telemaco end marker comes before the start marker".to_string(),
                )
            } else {
                MarkedSection::One(start, end + TELEMACO_END_MARKER.len())
            }
        }
        _ => MarkedSection::Malformed(format!(
            "found {} Telemaco start and {} end markers, expected one of each",
            starts, ends
        )),
    }
}

fn malformed(msg: String) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("{}; fix or remove them by hand and re-run", msg),
    )
}
