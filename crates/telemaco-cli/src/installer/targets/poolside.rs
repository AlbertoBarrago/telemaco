use std::fs;
use std::path::PathBuf;

use super::common::*;

/// Where Poolside keeps its settings.
///
/// Personal settings and personal `AGENTS.md` live in
/// `~/.config/poolside/` and nowhere else; project settings live in
/// `.poolside/settings.yaml` (docs.poolside.ai/settings-file-reference,
/// /agent-instructions). The `~/.poolside` fallback this used on a machine
/// without the config directory yet wrote where nothing reads.
fn config_dir(loc: &Location, home: &PathBuf) -> PathBuf {
    match loc {
        Location::Global => home.join(".config").join("poolside"),
        Location::Folder(folder) => folder.join(".poolside"),
    }
}

/// Where earlier versions wrote a global config, cleaned up on the way out.
fn legacy_global_dir(loc: &Location, home: &PathBuf) -> Option<PathBuf> {
    match loc {
        Location::Global => Some(home.join(".poolside")),
        Location::Folder(_) => None,
    }
}

pub fn detect(loc: &Location, home: Option<&PathBuf>) -> DetectionResult {
    let h = home.cloned().unwrap_or_else(|| PathBuf::from("."));
    let dir = config_dir(loc, &h);
    let config_path = dir.join("settings.yaml");
    let installed = dir.exists()
        || config_path.exists()
        || legacy_global_dir(loc, &h).map_or(false, |d| d.exists());
    let mut already_configured = false;
    if config_path.exists() {
        if let Ok(content) = fs::read_to_string(&config_path) {
            already_configured = content.contains("telemaco");
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
                if folder.join(".poolside").exists() { markers.push(".poolside/"); }
            }
            if markers.is_empty() { "detected".to_string() } else { markers.join(", ") }
        }
    } else {
        String::new()
    };
    DetectionResult {
        installed,
        already_configured,
        config_path: Some(config_path),
        hint,
    }
}

/// Rewrites our guard hook to the command we would write today, for the same
/// reason: a stale one names a binary or a flag that no longer exists.
fn refresh_guard(content: &str, fresh_item: &str) -> Option<String> {
    refresh_named_hook(content, "telemaco-guard", fresh_item)
}

/// Rewrites one of our named hooks in place, keeping its position in the list:
/// "A named hook declared again at a more specific level replaces the earlier
/// declaration in its original position" (docs.poolside.ai/hooks).
fn refresh_named_hook(content: &str, name: &str, fresh_item: &str) -> Option<String> {
    let marker = format!("- name: {}", name);
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.iter().position(|l| l.trim().starts_with(&marker))?;
    let end = block_extent(&lines, start);
    let head = indent_of(lines[start]);
    let block = reindent_block(fresh_item, head, head + 2);
    if lines[start..end].join("\n") == block {
        return None;
    }

    let mut out: Vec<String> = lines[..start].iter().map(|l| l.to_string()).collect();
    out.extend(block.lines().map(|l| l.to_string()));
    out.extend(lines[end..].iter().map(|l| l.to_string()));
    Some(format!("{}\n", out.join("\n")))
}

/// Poolside keeps its config in YAML, so the entry is built as text.
pub fn mcp_yaml_entry(binary_path: &str, stealth: bool) -> String {
    yaml_mcp_entry("telemaco", binary_path, &stdio_mcp_args(stealth))
}



/// Takes our entries back out of a Poolside settings file.
fn remove_our_yaml(out: &mut Outcome, settings_path: &std::path::Path) {
    if !settings_path.exists() {
        return;
    }
    let Some(original) = out.load_text(settings_path) else {
        return;
    };
    let content = original.clone();
    let (content, removed_mcp) = remove_yaml_block(&content, |l| l.trim() == "telemaco:");
    let (content, removed_hook) =
        remove_yaml_block(&content, |l| l.trim().starts_with("- name: telemaco-guard"));
    let (content, removed_context) =
        remove_yaml_block(&content, |l| l.trim().starts_with("- name: telemaco-context"));
    if !(removed_mcp || removed_hook || removed_context) {
        return;
    }
    let pruned = prune_empty_yaml_keys(
        &content,
        &["mcp_servers", "hooks", "PreToolUse", "UserPromptSubmit"],
    );
    if pruned.trim().is_empty() {
        // Nothing of the user's left; do not leave a 0-byte file.
        out.remove_config_file(settings_path, "");
    } else {
        let rebuilt = with_line_ending(&original, &format!("{}\n", pruned.trim_end()));
        out.write_text(settings_path, &rebuilt, Action::Updated);
    }
}

pub fn install(loc: &Location, opts: &TargetInstallOptions, home: &PathBuf) -> TargetResult {
    let mut out = Outcome::new(opts.dry_run);

    let dir = config_dir(loc, home);
    let settings_path = dir.join("settings.yaml");
    let mcp_entry = mcp_yaml_entry(&opts.binary_path, opts.stealth);
    // The guard runs a shell command that exits non-zero, so it actually
    // refuses the call. It used to run `telemaco prompt-hook`, which reads a
    // prompt payload on stdin and exits 0, blocking nothing.
    let guard_item = format!(
        "- name: telemaco-guard\n  matcher: \"web_search|web_fetch\"\n  command: \"{}\"\n  timeout: 10",
        WEB_BLOCK_COMMAND.replace('\\', "\\\\").replace('"', "\\\"")
    );
    // `UserPromptSubmit` is the event that can "inject context", and a matcher
    // is expected on every event even where it is ignored: "Other events ignore
    // it, so use `matcher: \"*\"`" (docs.poolside.ai/hooks).
    let context_item = format!(
        "- name: telemaco-context\n  matcher: \"*\"\n  command: \"{}\"\n  timeout: 30",
        prompt_hook_command_poolside(&opts.binary_path)
    );

    let has_mcp_entry = |c: &str| c.lines().any(|l| l.trim() == "telemaco:");
    let has_guard = |c: &str| c.contains("telemaco-guard");
    let has_context = |c: &str| c.contains("telemaco-context");

    if !settings_path.exists() {
        let mut content = format!("mcp_servers:\n{}\n", indent_block(&mcp_entry, 2));
        content.push_str("\nhooks:\n");
        if opts.block_builtin_web {
            content.push_str(&format!("  PreToolUse:\n{}\n", indent_block(&guard_item, 4)));
        }
        content.push_str(&format!(
            "  UserPromptSubmit:\n{}\n",
            indent_block(&context_item, 4)
        ));
        out.write_text(&settings_path, &content, Action::Created);
    } else if let Some(mut content) = out.load_text(&settings_path) {
        let original = content.clone();
        let mut modified = false;
        if has_mcp_entry(&content) {
            if let Some(updated) = refresh_yaml_entry(&content, "telemaco", &mcp_entry) {
                if updated != content {
                    content = updated;
                    modified = true;
                }
            }
        } else {
            match upsert_yaml_path(&content, &["mcp_servers"], &mcp_entry) {
                Ok(updated) => {
                    content = updated;
                    modified = true;
                }
                Err(e) => out.note(format!("{}: {}", settings_path.display(), e)),
            }
        }
        if opts.block_builtin_web && has_guard(&content) {
            if let Some(updated) = refresh_guard(&content, &guard_item) {
                content = updated;
                modified = true;
            }
        } else if opts.block_builtin_web {
            match upsert_yaml_path(&content, &["hooks", "PreToolUse"], &guard_item) {
                Ok(updated) => {
                    content = updated;
                    modified = true;
                }
                Err(e) => out.note(format!("{}: {}", settings_path.display(), e)),
            }
        } else if !opts.block_builtin_web && has_guard(&content) {
            // Reinstalling with the guard declined has to take back the one an
            // earlier install added, the same way Claude Code's does.
            let (without, removed) =
                remove_yaml_block(&content, |l| l.trim().starts_with("- name: telemaco-guard"));
            if removed {
                content = prune_empty_yaml_keys(&without, &["hooks", "PreToolUse"]);
                modified = true;
            }
        }
        if has_context(&content) {
            if let Some(updated) = refresh_named_hook(&content, "telemaco-context", &context_item) {
                if updated != content {
                    content = updated;
                    modified = true;
                }
            }
        } else {
            match upsert_yaml_path(&content, &["hooks", "UserPromptSubmit"], &context_item) {
                Ok(updated) => {
                    content = updated;
                    modified = true;
                }
                Err(e) => out.note(format!("{}: {}", settings_path.display(), e)),
            }
        }

        if modified {
            let content = with_line_ending(&original, &content);
            out.write_text(&settings_path, &content, Action::Updated);
        } else {
            out.push(FileResult { path: settings_path, action: Action::Unchanged });
        }
    }

    let instructions_path = match loc {
        Location::Global => dir.join("AGENTS.md"),
        Location::Folder(folder) => folder.join("AGENTS.md"),
    };
    update_instructions(&mut out, &instructions_path, opts.stealth);

    out.finish(TargetId::Poolside)
}

pub fn uninstall(loc: &Location, home: &PathBuf, dry_run: bool) -> TargetResult {
    let mut out = Outcome::new(dry_run);

    let dir = config_dir(loc, home);
    if let Some(legacy) = legacy_global_dir(loc, home) {
        remove_our_yaml(&mut out, &legacy.join("settings.yaml"));
        remove_instructions(&mut out, &legacy.join("AGENTS.md"));
    }
    let settings_path = dir.join("settings.yaml");
    remove_our_yaml(&mut out, &settings_path);

    let instructions_path = match loc {
        Location::Global => dir.join("AGENTS.md"),
        Location::Folder(folder) => folder.join("AGENTS.md"),
    };
    remove_instructions(&mut out, &instructions_path);

    out.finish(TargetId::Poolside)
}
