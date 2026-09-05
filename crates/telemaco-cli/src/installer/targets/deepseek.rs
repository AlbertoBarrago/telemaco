use std::fs;
use std::path::PathBuf;

use super::common::*;

/// The harness home: `$DSH_HOME`, else `~/.dsh`.
///
/// Every path the harness reads is relative to it - "defaults to `$DSH_HOME`
/// or `~/.dsh`" appears on the instruction loader, the skill roots and the
/// patch layer alike (deepseek-ai/deepseek-harness docs/config-catalog.md).
fn dsh_home(home: &PathBuf) -> PathBuf {
    home_env_var("DSH_HOME").unwrap_or_else(|| home.join(".dsh"))
}

/// The user patch layer, where a plugin is turned on for every profile.
///
/// "To keep the selection across runs, merge the chosen file's single `insert`
/// patch into a user patch layer - `$DSH_HOME/profiles/<profile>/cordis.patch.yml`
/// for one profile, or `$DSH_HOME/cordis.patch.yml` for every profile on the
/// machine" (docs/user/guide/mcp-memory.md). The file is a sequence of patch
/// operations, so ours is appended to it rather than written over it: "Do not
/// copy over an existing file: it may already contain unrelated user patches."
fn patch_path(home: &PathBuf) -> PathBuf {
    dsh_home(home).join("cordis.patch.yml")
}

const PATCH_ID: &str = "telemaco-mcp";

/// The MCP server, in the shape `@deepseek-ai/dsh-mcp-client` declares:
/// `serverName` (`[A-Za-z0-9_-]{1,32}`), `transport`, `command`, `args`
/// (docs/config-catalog.md, `StdioConfig`).
fn mcp_patch_item(binary_path: &str, stealth: bool) -> String {
    let mut item = String::from("- insert:\n");
    item.push_str(&format!("    - id: {}\n", PATCH_ID));
    item.push_str("      name: \"@deepseek-ai/dsh-mcp-client\"\n");
    item.push_str("      config:\n");
    item.push_str("        serverName: telemaco\n");
    item.push_str("        transport: stdio\n");
    item.push_str(&format!("        command: \"{}\"\n", binary_path));
    item.push_str("        args:\n");
    for arg in stdio_mcp_args(stealth) {
        item.push_str(&format!("          - \"{}\"\n", arg));
    }
    item
}

/// Whether a line carries our patch id, in a nested sequence entry
/// (`    - id: telemaco-mcp`) or a plain mapping key (`      id: telemaco-mcp`).
fn line_has_patch_id(line: &str) -> bool {
    let trimmed = line.trim();
    let trimmed = trimmed.strip_prefix("- ").unwrap_or(trimmed);
    trimmed.trim() == format!("id: {}", PATCH_ID)
}

/// Line ranges of the top-level sequence items, one per patch operation.
fn top_level_items(lines: &[&str]) -> Vec<(usize, usize)> {
    let mut items: Vec<(usize, usize)> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.starts_with("- ") || *line == "-" {
            if let Some(last) = items.last_mut() {
                last.1 = i;
            }
            items.push((i, lines.len()));
        }
    }
    items
}

/// Replaces our patch operation with the one we would write today, or appends
/// it. A stale copy names a binary that has moved, the same way every other
/// target's entry would.
fn upsert_patch_item(content: &str, fresh_item: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let ours = top_level_items(&lines)
        .into_iter()
        .find(|(start, end)| lines[*start..*end].iter().any(|l| line_has_patch_id(l)));

    match ours {
        Some((start, end)) => {
            let mut out: Vec<String> = lines[..start].iter().map(|l| l.to_string()).collect();
            out.extend(fresh_item.lines().map(|l| l.to_string()));
            out.extend(lines[end..].iter().map(|l| l.to_string()));
            format!("{}\n", out.join("\n"))
        }
        None => {
            let mut out = content.trim_end().to_string();
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(fresh_item);
            out
        }
    }
}

/// Drops our patch operation, leaving every other one untouched.
fn remove_patch_item(content: &str) -> (String, bool) {
    let lines: Vec<&str> = content.lines().collect();
    let ours = top_level_items(&lines)
        .into_iter()
        .find(|(start, end)| lines[*start..*end].iter().any(|l| line_has_patch_id(l)));

    match ours {
        Some((start, end)) => {
            let mut out: Vec<String> = lines[..start].iter().map(|l| l.to_string()).collect();
            out.extend(lines[end..].iter().map(|l| l.to_string()));
            let joined = out.join("\n");
            if joined.trim().is_empty() {
                (String::new(), true)
            } else {
                (format!("{}\n", joined.trim_end()), true)
            }
        }
        None => (content.to_string(), false),
    }
}

/// Whether anything but comments and blank lines is left.
fn is_effectively_empty(content: &str) -> bool {
    content
        .lines()
        .all(|l| l.trim().is_empty() || l.trim_start().starts_with('#'))
}

pub fn detect(loc: &Location, home: Option<&PathBuf>) -> DetectionResult {
    let h = home.cloned().unwrap_or_else(|| PathBuf::from("."));
    let (dsh_dir, instr_path, installed) = match loc {
        Location::Global => {
            let dsh_dir = dsh_home(&h);
            let instr = dsh_dir.join("AGENTS.md");
            let installed = dsh_dir.exists();
            (dsh_dir, instr, installed)
        }
        Location::Folder(folder) => {
            let dsh_dir = folder.join(".dsh");
            let instr = folder.join("AGENTS.md");
            let installed = dsh_dir.exists();
            (dsh_dir, instr, installed)
        }
    };
    let mut already_configured = false;
    if instr_path.exists() {
        if let Ok(content) = fs::read_to_string(&instr_path) {
            already_configured = content.contains("<!-- TELEMACO_START -->");
        }
    }
    let hooks_path = dsh_dir.join("hooks.json");
    if !already_configured && hooks_path.exists() {
        if let Ok(content) = fs::read_to_string(&hooks_path) {
            already_configured = text_has_telemaco_hook(&content);
        }
    }
    if !already_configured && loc.is_global() {
        let patch = patch_path(&h);
        if let Ok(content) = fs::read_to_string(&patch) {
            already_configured = content.contains(&format!("id: {}", PATCH_ID));
        }
    }
    let hint = if already_configured {
        "already configured".to_string()
    } else if installed {
        if loc.is_global() {
            "detected".to_string()
        } else {
            let mut markers = Vec::new();
            if let Location::Folder(folder) = loc {
                if folder.join(".dsh").exists() { markers.push(".dsh/"); }
            }
            if markers.is_empty() { "detected".to_string() } else { markers.join(", ") }
        }
    } else {
        String::new()
    };
    DetectionResult {
        installed,
        already_configured,
        config_path: Some(instr_path),
        hint,
    }
}

pub fn install(loc: &Location, opts: &TargetInstallOptions, home: &PathBuf) -> TargetResult {
    let mut out = Outcome::new(opts.dry_run);

    let dsh_dir = match loc {
        Location::Global => dsh_home(home),
        Location::Folder(folder) => folder.join(".dsh"),
    };

    // The MCP server goes in the user patch layer, which is the documented way
    // to turn a plugin on for every run: the memory guide has the user merge
    // exactly this kind of `insert` into it
    // (docs/user/guide/mcp-memory.md). There is no project-level patch layer,
    // so a folder install gets the instructions block only.
    if loc.is_global() {
        let patch = patch_path(home);
        let fresh = mcp_patch_item(&opts.binary_path, opts.stealth);
        if !patch.exists() {
            out.write_text(&patch, &fresh, Action::Created);
        } else if let Some(original) = out.load_text(&patch) {
            let updated = upsert_patch_item(&original, &fresh);
            if updated != original {
                let rebuilt = with_line_ending(&original, &updated);
                out.write_text(&patch, &rebuilt, Action::Updated);
            } else {
                out.push(FileResult { path: patch, action: Action::Unchanged });
            }
        }
    }

    // The file is in Claude Code's shape, which is what dsh's hook plugin
    // reads, but the harness does not discover it on its own: the plugin takes
    // an explicit `configPath` (deepseek-ai/deepseek-harness
    // docs/config-catalog.md, `@deepseek-ai/dsh-hooks-claude-code`). Say so
    // rather than let the user believe the hook is live.
    let hooks_path = dsh_dir.join("hooks.json");
    let hooks_existed = hooks_path.exists();
    if let Some(mut hooks_json) = out.load_json(&hooks_path) {
        if add_user_prompt_hook(&mut hooks_json, &prompt_hook_command(&opts.binary_path)) {
            let action = if hooks_existed { Action::Updated } else { Action::Created };
            out.write_json(&hooks_path, &hooks_json, action);
        }
        // Said on every run, not only the one that writes the file: an
        // unchanged hook is just as inert until the plugin points at it.
        out.note(format!(
            "The prompt hook runs once you enable the \
             @deepseek-ai/dsh-hooks-claude-code plugin with configPath: {}. \
             The instructions block works without it.",
            hooks_path.display()
        ));
    }

    let instructions_path = match loc {
        Location::Global => dsh_home(home).join("AGENTS.md"),
        Location::Folder(folder) => folder.join("AGENTS.md"),
    };
    update_instructions(&mut out, &instructions_path, opts.stealth);

    out.finish(TargetId::DeepSeek)
}

pub fn uninstall(loc: &Location, home: &PathBuf, dry_run: bool) -> TargetResult {
    let mut out = Outcome::new(dry_run);

    let dsh_dir = match loc {
        Location::Global => dsh_home(home),
        Location::Folder(folder) => folder.join(".dsh"),
    };

    if loc.is_global() {
        let patch = patch_path(home);
        if patch.exists() {
            if let Some(original) = out.load_text(&patch) {
                let (content, removed) = remove_patch_item(&original);
                if removed {
                    if is_effectively_empty(&content) {
                        out.remove_config_file(&patch, "");
                    } else {
                        let rebuilt = with_line_ending(&original, &content);
                        out.write_text(&patch, &rebuilt, Action::Removed);
                    }
                }
            }
        }
    }

    let hooks_path = dsh_dir.join("hooks.json");
    if hooks_path.exists() {
        if let Some(mut hooks_json) = out.load_json(&hooks_path) {
            if remove_user_prompt_hook(&mut hooks_json) {
                out.write_json_or_remove(&hooks_path, &hooks_json, Action::Updated);
            }
        }
    }

    let instructions_path = match loc {
        Location::Global => dsh_home(home).join("AGENTS.md"),
        Location::Folder(folder) => folder.join("AGENTS.md"),
    };
    remove_instructions(&mut out, &instructions_path);

    out.finish(TargetId::DeepSeek)
}
