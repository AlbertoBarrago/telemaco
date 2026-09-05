use std::fs;
use std::path::{Path, PathBuf};
use serde_json::json;

use super::common::*;

pub fn detect(loc: &Location, home: Option<&PathBuf>) -> DetectionResult {
    let (mcp_path, cursor_dir, cursorrules) = match loc {
        Location::Global => {
            let h = home.cloned().unwrap_or_else(|| PathBuf::from("."));
            (h.join(".cursor").join("mcp.json"), h.join(".cursor"), h.join(".cursorrules"))
        }
        Location::Folder(folder) => {
            (folder.join(".cursor").join("mcp.json"), folder.join(".cursor"), folder.join(".cursorrules"))
        }
    };
    let installed = cursor_dir.exists() || mcp_path.exists() || cursorrules.exists();
    let mut already_configured = false;
    if mcp_path.exists() {
        let json = read_json_file(&mcp_path);
        if let Some(servers) = json.get("mcpServers").and_then(|v| v.as_object()) {
            already_configured = servers.contains_key("telemaco");
        }
    }
    let hooks_path = cursor_dir.join("hooks.json");
    if !already_configured && hooks_path.exists() {
        if let Ok(content) = fs::read_to_string(&hooks_path) {
            already_configured = text_has_telemaco_hook(&content);
        }
    }
    let hint = if already_configured {
        "already configured".to_string()
    } else if installed {
        if loc.is_global() {
            "detected".to_string()
        } else {
            let mut markers = Vec::new();
            if cursor_dir.exists() { markers.push(".cursor/"); }
            if cursorrules.exists() { markers.push(".cursorrules"); }
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

/// Adds or removes our `sessionStart` hook in a Cursor `hooks.json`.
///
/// This is how a global install delivers the directive at all. Cursor's global
/// instructions are User Rules, which live in the Customize panel and not in
/// any file we can write, and `~/.cursor/rules/` is not a path Cursor
/// documents reading (cursor.com/docs/rules). A user-level `~/.cursor/hooks.json`
/// is documented, and `sessionStart` returns `additional_context`, which Cursor
/// adds to "the conversation's initial system context" (cursor.com/docs/hooks).
///
/// `beforeSubmitPrompt` cannot do this: its only outputs are `continue` and
/// `user_message`, so it can block a prompt but never add to one.
fn sync_session_hook(out: &mut Outcome, path: &Path, command: &str, enabled: bool) {
    let existed = path.exists();
    if !enabled && !existed {
        return;
    }
    let Some(mut json) = out.load_json(path) else {
        return;
    };
    let before = json.clone();

    {
        let Some(obj) = json.as_object_mut() else {
            return;
        };
        if enabled {
            obj.insert("version".to_string(), json!(1));
        }
        let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
        let Some(hooks_map) = hooks.as_object_mut() else {
            return;
        };
        let entries = hooks_map.entry("sessionStart").or_insert_with(|| json!([]));
        let Some(arr) = entries.as_array_mut() else {
            return;
        };
        arr.retain(|e| {
            !e.get("command")
                .and_then(|c| c.as_str())
                .map_or(false, is_telemaco_hook_command)
        });
        if enabled {
            arr.push(json!({ "command": command }));
        }
        if arr.is_empty() {
            hooks_map.remove("sessionStart");
        }
        if hooks_map.is_empty() {
            obj.remove("hooks");
            // `version` alone is not a hooks file, it is a husk.
            obj.remove("version");
        }
    }

    if json_deep_equal(&before, &json) {
        if existed {
            out.push(FileResult { path: path.to_path_buf(), action: Action::Unchanged });
        }
        return;
    }
    let action = if existed { Action::Updated } else { Action::Created };
    out.write_json_or_remove(path, &json, action);
}

pub fn install(loc: &Location, opts: &TargetInstallOptions, home: &PathBuf) -> TargetResult {
    let mut out = Outcome::new(opts.dry_run);

    // 1. MCP entry in ~/.cursor/mcp.json or <folder>/.cursor/mcp.json
    let mcp_path = match loc {
        Location::Global => home.join(".cursor").join("mcp.json"),
        Location::Folder(folder) => folder.join(".cursor").join("mcp.json"),
    };
    // `type` is listed as required for a STDIO server in Cursor's own field
    // table, even though its examples leave it out (cursor.com/docs/mcp).
    upsert_mcp_server(
        &mut out,
        &mcp_path,
        "telemaco",
        stdio_typed_mcp_entry(&opts.binary_path, opts.stealth),
    );

    // 2. Instructions.
    //
    // A project gets an always-applied rule, the documented route: a `.mdc` in
    // `.cursor/rules` with `alwaysApply: true` is "applied to every chat
    // session" (cursor.com/docs/rules). A plain `.md` there is ignored, so the
    // extension matters.
    //
    // Globally there is no such file. User Rules are typed into the Customize
    // panel, and `~/.cursor/rules/` appears nowhere in the docs, so writing one
    // there was writing into the void. The user-level hook below is the
    // documented way in.
    if let Location::Folder(folder) = loc {
        let rule_path = folder.join(".cursor").join("rules").join("telemaco.mdc");
        let rule_content = format!(
            "---\ndescription: Enforce using Telemaco for all web browsing, searching and scraping\nalwaysApply: true\n---\n\n{}",
            get_instructions_block(opts.stealth)
        );
        let rule_existed = rule_path.exists();
        if rule_existed && fs::read_to_string(&rule_path).ok().as_deref() == Some(rule_content.as_str()) {
            out.push(FileResult { path: rule_path, action: Action::Unchanged });
        } else {
            let action = if rule_existed { Action::Updated } else { Action::Created };
            out.write_text(&rule_path, &rule_content, action);
        }
    } else {
        let hooks_path = home.join(".cursor").join("hooks.json");
        sync_session_hook(
            &mut out,
            &hooks_path,
            &prompt_hook_command_cursor(&opts.binary_path),
            true,
        );
    }

    if let Location::Folder(folder) = loc {
        let cr = folder.join(".cursorrules");
        if cr.exists() {
            update_instructions(&mut out, &cr, opts.stealth);
        }
    }

    out.finish(TargetId::Cursor)
}

pub fn uninstall(loc: &Location, home: &PathBuf, dry_run: bool) -> TargetResult {
    let mut out = Outcome::new(dry_run);

    let mcp_path = match loc {
        Location::Global => home.join(".cursor").join("mcp.json"),
        Location::Folder(folder) => folder.join(".cursor").join("mcp.json"),
    };
    remove_mcp_server(&mut out, &mcp_path, "telemaco");

    // The global rule file is one earlier versions wrote into a path Cursor
    // does not read; it still has to be cleaned up.
    let rule_path = match loc {
        Location::Global => home.join(".cursor").join("rules").join("telemaco.mdc"),
        Location::Folder(folder) => folder.join(".cursor").join("rules").join("telemaco.mdc"),
    };
    if rule_path.exists() {
        out.remove_file(&rule_path);
    }

    if let Location::Folder(folder) = loc {
        let cr = folder.join(".cursorrules");
        if cr.exists() {
            remove_instructions(&mut out, &cr);
        }
    }

    let hooks_path = match loc {
        Location::Global => home.join(".cursor").join("hooks.json"),
        Location::Folder(folder) => folder.join(".cursor").join("hooks.json"),
    };
    sync_session_hook(&mut out, &hooks_path, "", false);
    if hooks_path.exists() {
        // A `UserPromptSubmit` group is Claude Code's shape, which an earlier
        // version wrote here by mistake.
        if let Some(mut hooks_json) = out.load_json(&hooks_path) {
            if remove_user_prompt_hook(&mut hooks_json) {
                out.write_json_or_remove(&hooks_path, &hooks_json, Action::Updated);
            }
        }
    }

    out.finish(TargetId::Cursor)
}
