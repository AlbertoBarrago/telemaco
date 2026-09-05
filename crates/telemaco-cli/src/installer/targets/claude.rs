use std::path::{Path, PathBuf};
use serde_json::json;

use super::common::*;

/// Claude Code's own config directory: `~/.claude` unless `$CLAUDE_CONFIG_DIR`
/// says otherwise.
///
/// "To keep the home-directory files somewhere else, set
/// `CLAUDE_CONFIG_DIR`; Claude Code then stores your settings, session
/// history, and plugins there instead" (docs.claude.com/en/docs/claude-code/settings).
/// With it set, everything written under `~/.claude` is read by nothing - the
/// same trap `CODEX_HOME` and `HERMES_HOME` were.
fn claude_config_dir(home: &PathBuf) -> PathBuf {
    home_env_var("CLAUDE_CONFIG_DIR").unwrap_or_else(|| home.join(".claude"))
}

/// `~/.claude.json`, Claude Code's own state file: "Claude Code also keeps a
/// fifth file, `~/.claude.json`, that it writes for itself; [...] It holds
/// your sign-in session, MCP server configurations, per-project state such as
/// trust decisions, and the global config keys" (same page).
///
/// Unlike `settings.json`, this one is *not* nested one level inside the
/// config directory by default - it sits next to `.claude/` at `$HOME`. But
/// once `CLAUDE_CONFIG_DIR` relocates that directory, it moves inside it: on a
/// machine with `CLAUDE_CONFIG_DIR` set, this file lives at
/// `$CLAUDE_CONFIG_DIR/.claude.json`, confirmed against a real Claude Code
/// session's own live trust state, not only the docs' wording.
fn claude_json_path(home: &PathBuf) -> PathBuf {
    match home_env_var("CLAUDE_CONFIG_DIR") {
        Some(dir) => dir.join(".claude.json"),
        None => home.join(".claude.json"),
    }
}

pub fn detect(loc: &Location, home: Option<&PathBuf>) -> DetectionResult {
    let (mcp_path, config_dir, claude_md) = match loc {
        Location::Global => {
            let h = home.cloned().unwrap_or_else(|| PathBuf::from("."));
            let config_dir = claude_config_dir(&h);
            (claude_json_path(&h), config_dir.clone(), config_dir.join("CLAUDE.md"))
        }
        Location::Folder(folder) => {
            let mcp = if folder.join(".mcp.json").exists() {
                folder.join(".mcp.json")
            } else if folder.join(".claude.json").exists() {
                folder.join(".claude.json")
            } else {
                folder.join(".mcp.json")
            };
            (mcp, folder.join(".claude"), project_instructions_path(folder))
        }
    };
    let installed = config_dir.exists() || mcp_path.exists() || claude_md.exists();
    let mut already_configured = false;
    if mcp_path.exists() {
        let json = read_json_file(&mcp_path);
        if let Some(servers) = json.get("mcpServers").and_then(|v| v.as_object()) {
            already_configured = servers.contains_key("telemaco");
        }
    }
    let hint = if already_configured {
        "already configured".to_string()
    } else if installed {
        if loc.is_global() {
            "detected".to_string()
        } else {
            let mut markers = Vec::new();
            if config_dir.exists() { markers.push(".claude/"); }
            if mcp_path.exists() { markers.push(".mcp.json"); }
            if claude_md.exists() { markers.push("CLAUDE.md"); }
            if markers.is_empty() { "detected".to_string() } else { markers.join(", ") }
        }
    } else {
        String::new()
    };
    DetectionResult {
        installed,
        already_configured,
        config_path: Some(mcp_path),
        hint,
    }
}

/// Where a project keeps its instructions.
///
/// A project CLAUDE.md lives at `./CLAUDE.md` or `./.claude/CLAUDE.md`, and
/// every discovered file is concatenated rather than one overriding another
/// (docs.claude.com/en/docs/claude-code/memory). So both work, and the one the
/// project already uses wins: a repository that keeps its instructions in
/// `.claude/` has no use for a second file appearing at its root.
fn project_instructions_path(folder: &Path) -> PathBuf {
    let nested = folder.join(".claude").join("CLAUDE.md");
    if nested.exists() {
        nested
    } else {
        folder.join("CLAUDE.md")
    }
}

/// Both project instruction files, for cleanup.
fn all_project_instructions(folder: &Path) -> Vec<PathBuf> {
    vec![
        folder.join("CLAUDE.md"),
        folder.join(".claude").join("CLAUDE.md"),
    ]
}

/// The allow rule that auto-approves Telemaco's MCP tools.
///
/// A glob is only accepted after a literal `mcp__<server>__` prefix, which this
/// has (docs.claude.com/en/docs/claude-code/permissions).
const MCP_PERMISSION: &str = "mcp__telemaco__*";

/// Drops the top-level `autoApprove` array earlier versions wrote.
///
/// Claude Code has no such setting: it reads `permissions.allow`, so the key
/// sat in settings.json auto-approving nothing. Only our own entry is taken
/// out, and the key goes with it once it is empty.
fn remove_legacy_auto_approve(settings_json: &mut serde_json::Value) -> bool {
    let Some(obj) = settings_json.as_object_mut() else {
        return false;
    };
    let Some(arr) = obj.get_mut("autoApprove").and_then(|v| v.as_array_mut()) else {
        return false;
    };
    let before = arr.len();
    arr.retain(|item| item.as_str() != Some(MCP_PERMISSION));
    let changed = arr.len() != before;
    if arr.is_empty() {
        obj.remove("autoApprove");
    }
    changed
}

/// Adds or removes auto-approval for Telemaco's MCP tools.
///
/// The removal half matters: `--no-permissions` on an install that already
/// granted it has to take the grant back, not merely stop re-adding it.
fn sync_permission_allow(settings_json: &mut serde_json::Value, enabled: bool) -> bool {
    let mut changed = remove_legacy_auto_approve(settings_json);

    if !enabled {
        if let Some(arr) = settings_json
            .get_mut("permissions")
            .and_then(|p| p.get_mut("allow"))
            .and_then(|v| v.as_array_mut())
        {
            let before = arr.len();
            arr.retain(|item| item.as_str() != Some(MCP_PERMISSION));
            changed |= arr.len() != before;
        }
        return changed;
    }

    let permissions = settings_json
        .as_object_mut()
        .expect("load_json guarantees an object")
        .entry("permissions")
        .or_insert_with(|| json!({}));
    let Some(perm_obj) = permissions.as_object_mut() else {
        return changed;
    };
    let allow = perm_obj.entry("allow").or_insert_with(|| json!([]));
    let Some(arr) = allow.as_array_mut() else {
        return changed;
    };
    if arr.iter().any(|item| item.as_str() == Some(MCP_PERMISSION)) {
        return changed;
    }
    arr.push(json!(MCP_PERMISSION));
    true
}

/// Approves our own server from the project's `.mcp.json`.
///
/// A project-scoped server is not connected until it is approved: Claude Code
/// shows it as "Pending approval" and its tools are simply absent from the
/// session, so writing `.mcp.json` alone left the install looking done and
/// doing nothing (docs.claude.com/en/docs/claude-code/mcp, project scope).
/// `enabledMcpjsonServers` is the key Claude Code itself writes when you
/// approve one, and `.claude/settings.local.json` is where it writes it: that
/// file is personal and untracked, so the approval is ours alone rather than
/// something every teammate who clones the repository inherits, and it is
/// honored even in a folder whose trust dialog has not been accepted
/// (settings reference, `enabledMcpjsonServers`).
///
/// User-scoped servers in `~/.claude.json` need no approval, so this is for
/// project installs only.
///
/// The approval does not take effect before the folder is trusted: in a folder
/// whose trust dialog has not been accepted, `claude mcp list` and `claude mcp
/// get` ignore these keys, and checking with the real CLI in a scratch project
/// shows `Pending approval` for `enabledMcpjsonServers` and for the blanket
/// `enableAllProjectMcpServers` alike. Nothing an installer writes can accept
/// that dialog, so the note says what is left to do.
fn sync_project_mcp_approval(out: &mut Outcome, folder: &Path, enabled: bool) {
    let path = folder.join(".claude").join("settings.local.json");
    let existed = path.exists();
    if !enabled && !existed {
        return;
    }
    let Some(mut json) = out.load_json(&path) else {
        return;
    };
    let Some(obj) = json.as_object_mut() else {
        return;
    };

    let changed = if enabled {
        let list = obj
            .entry("enabledMcpjsonServers")
            .or_insert_with(|| json!([]));
        match list.as_array_mut() {
            Some(arr) if !arr.iter().any(|s| s.as_str() == Some("telemaco")) => {
                arr.push(json!("telemaco"));
                true
            }
            _ => false,
        }
    } else {
        match obj
            .get_mut("enabledMcpjsonServers")
            .and_then(|v| v.as_array_mut())
        {
            Some(arr) => {
                let before = arr.len();
                arr.retain(|s| s.as_str() != Some("telemaco"));
                arr.len() != before
            }
            None => false,
        }
    };
    if !changed {
        return;
    }

    if enabled && !existed {
        out.note(format!(
            "Approved the telemaco server in {}. Claude Code applies it once you accept the \
             folder's trust dialog on the first interactive run. That file is personal: add \
             it to .gitignore if git does not ignore it already.",
            path.display()
        ));
    }
    let action = if existed { Action::Updated } else { Action::Created };
    out.write_json_or_remove(&path, &json, action);
}

/// True when a PreToolUse group is the guard *we* installed.
///
/// Matching on the matcher alone is not enough: a user may well have written
/// their own `WebSearch|WebFetch` guard, and uninstalling Telemaco must not
/// take it away with it.
fn is_our_web_block(group: &serde_json::Value) -> bool {
    const MATCHER: &str = "WebSearch|WebFetch";
    if group.get("matcher").and_then(|m| m.as_str()) != Some(MATCHER) {
        return false;
    }
    group
        .get("hooks")
        .and_then(|h| h.as_array())
        .map_or(false, |arr| {
            arr.iter().any(|entry| {
                entry
                    .get("command")
                    .and_then(|c| c.as_str())
                    .map_or(false, |cmd| cmd.to_lowercase().contains("telemaco"))
            })
        })
}

/// Adds or removes the PreToolUse guard that refuses Claude Code's built-in
/// WebSearch and WebFetch. Returns whether the settings changed.
///
/// Removing on `enabled == false` matters: reinstalling with the guard turned
/// off has to undo an earlier install that turned it on.
fn sync_web_block(settings_json: &mut serde_json::Value, enabled: bool) -> bool {
    if !enabled {
        let Some(groups) = settings_json
            .get_mut("hooks")
            .and_then(|h| h.get_mut("PreToolUse"))
            .and_then(|p| p.as_array_mut())
        else {
            return false;
        };
        let before = groups.len();
        groups.retain(|g| !is_our_web_block(g));
        return groups.len() != before;
    }

    let hooks_obj = settings_json
        .as_object_mut()
        .expect("load_json guarantees an object")
        .entry("hooks")
        .or_insert_with(|| json!({}));
    let Some(hooks_map) = hooks_obj.as_object_mut() else {
        return false;
    };
    let pre_tool = hooks_map.entry("PreToolUse").or_insert_with(|| json!([]));
    let Some(groups) = pre_tool.as_array_mut() else {
        return false;
    };

    for g in groups.iter_mut() {
        if !is_our_web_block(g) {
            continue;
        }
        let mut changed = false;
        if let Some(first) = g.get_mut("hooks").and_then(|h| h.as_array_mut()).and_then(|a| a.first_mut()) {
            if first.get("command").and_then(|c| c.as_str()) != Some(WEB_BLOCK_COMMAND) {
                first["command"] = json!(WEB_BLOCK_COMMAND);
                changed = true;
            }
        }
        return changed;
    }

    groups.push(json!({
        "matcher": "WebSearch|WebFetch",
        "hooks": [
            {
                "type": "command",
                "command": WEB_BLOCK_COMMAND
            }
        ]
    }));
    true
}

pub fn install(loc: &Location, opts: &TargetInstallOptions, home: &PathBuf) -> TargetResult {
    let mut out = Outcome::new(opts.dry_run);

    // 1. MCP entry in ~/.claude.json (global) or <folder>/.mcp.json (folder)
    let mcp_path = match loc {
        Location::Global => claude_json_path(home),
        Location::Folder(folder) => folder.join(".mcp.json"),
    };
    upsert_mcp_server(
        &mut out,
        &mcp_path,
        "telemaco",
        stdio_typed_mcp_entry(&opts.binary_path, opts.stealth),
    );

    // 2. Settings.json (autoApprove + prompt-hook + PreToolUse)
    let settings_path = match loc {
        Location::Global => claude_config_dir(home).join("settings.json"),
        Location::Folder(folder) => folder.join(".claude").join("settings.json"),
    };
    let settings_existed = settings_path.exists();
    if let Some(mut settings_json) = out.load_json(&settings_path) {
        let mut modified = false;

        if sync_permission_allow(&mut settings_json, opts.auto_allow) {
            modified = true;
        }

        // Register UserPromptSubmit hook
        if add_user_prompt_hook(&mut settings_json, &prompt_hook_command(&opts.binary_path)) {
            modified = true;
        }

        // PreToolUse guard on the agent's own web tools
        if sync_web_block(&mut settings_json, opts.block_builtin_web) {
            modified = true;
        }

        if modified {
            let action = if settings_existed { Action::Updated } else { Action::Created };
            out.write_json(&settings_path, &settings_json, action);
        }
    }

    // 3. A project server has to be approved before Claude Code connects it.
    if let Location::Folder(folder) = loc {
        sync_project_mcp_approval(&mut out, folder, opts.auto_allow);
    }

    // 4. Instructions in CLAUDE.md
    let instructions_path = match loc {
        Location::Global => claude_config_dir(home).join("CLAUDE.md"),
        Location::Folder(folder) => project_instructions_path(folder),
    };
    update_instructions(&mut out, &instructions_path, opts.stealth);

    out.finish(TargetId::Claude)
}

pub fn uninstall(loc: &Location, home: &PathBuf, dry_run: bool) -> TargetResult {
    let mut out = Outcome::new(dry_run);

    let mcp_path = match loc {
        Location::Global => claude_json_path(home),
        Location::Folder(folder) => folder.join(".mcp.json"),
    };
    remove_mcp_server(&mut out, &mcp_path, "telemaco");

    let settings_path = match loc {
        Location::Global => claude_config_dir(home).join("settings.json"),
        Location::Folder(folder) => folder.join(".claude").join("settings.json"),
    };
    if settings_path.exists() {
        if let Some(mut settings_json) = out.load_json(&settings_path) {
            let mut modified = false;

            if sync_permission_allow(&mut settings_json, false) {
                modified = true;
            }

            if remove_user_prompt_hook(&mut settings_json) {
                modified = true;
            }

            if sync_web_block(&mut settings_json, false) {
                modified = true;
            }

            if modified {
                out.write_json_or_remove(&settings_path, &settings_json, Action::Updated);
            }
        }
    }

    if let Location::Folder(folder) = loc {
        sync_project_mcp_approval(&mut out, folder, false);
    }

    match loc {
        Location::Global => {
            remove_instructions(&mut out, &claude_config_dir(home).join("CLAUDE.md"))
        }
        Location::Folder(folder) => {
            for path in all_project_instructions(folder) {
                if path.exists() {
                    remove_instructions(&mut out, &path);
                }
            }
        }
    }

    out.finish(TargetId::Claude)
}
