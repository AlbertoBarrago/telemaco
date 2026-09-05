use std::fs;
use std::path::PathBuf;
use serde_json::json;

use super::common::*;

pub fn detect(loc: &Location, home: Option<&PathBuf>) -> DetectionResult {
    let (kiro_dir, mcp_path) = match loc {
        Location::Global => {
            let h = home.cloned().unwrap_or_else(|| PathBuf::from("."));
            let k_dir = h.join(".kiro");
            let mcp = k_dir.join("settings").join("mcp.json");
            (k_dir, mcp)
        }
        Location::Folder(folder) => {
            let k_dir = folder.join(".kiro");
            let mcp = k_dir.join("settings").join("mcp.json");
            (k_dir, mcp)
        }
    };
    let hooks_path = kiro_dir.join("hooks").join("telemaco.json");
    // Only `.kiro/` identifies Kiro. Looking for "kiro" inside AGENTS.md
    // matched every repository whose instructions merely mention the agent -
    // this one included - so `--target auto` configured Kiro for people who do
    // not run it. Same trap as the `.pi` check in the Pi target.
    let installed = kiro_dir.exists();

    let mut already_configured = false;
    if mcp_path.exists() {
        let json = read_json_file(&mcp_path);
        if let Some(servers) = json.get("mcpServers").and_then(|v| v.as_object()) {
            already_configured = servers.contains_key("telemaco");
        }
    }
    if !already_configured && hooks_path.exists() {
        // The file is ours by name, but an empty or hand-edited one is not a
        // working install.
        if let Ok(content) = fs::read_to_string(&hooks_path) {
            already_configured = text_has_telemaco_hook(&content);
        }
    }
    if !already_configured {
        let steering = match loc {
            Location::Global => kiro_dir.join("steering").join("telemaco.md"),
            Location::Folder(folder) => {
                if kiro_dir.join("steering").exists() {
                    kiro_dir.join("steering").join("telemaco.md")
                } else {
                    folder.join("AGENTS.md")
                }
            }
        };
        already_configured = fs::read_to_string(&steering)
            .map_or(false, |c| c.contains("<!-- TELEMACO_START -->"));
    }

    let hint = if already_configured {
        "already configured".to_string()
    } else if installed {
        if loc.is_global() {
            "detected".to_string()
        } else {
            let mut markers = Vec::new();
            if kiro_dir.exists() { markers.push(".kiro/"); }
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

/// Kiro's `autoApprove` list.
///
/// Always written, empty when the user declined: leaving the key out let an
/// earlier `["*"]` survive the merge, so `--no-permissions` revoked nothing.
pub fn mcp_entry(binary_path: &str, stealth: bool, auto_allow: bool) -> serde_json::Value {
    json!({
        "command": binary_path,
        "args": stdio_mcp_args(stealth),
        "disabled": false,
        "autoApprove": if auto_allow { json!(["*"]) } else { json!([]) }
    })
}

fn hook_command_of(entry: &serde_json::Value) -> Option<&str> {
    entry.get("action").and_then(|a| a.get("command")).and_then(|c| c.as_str())
}

pub fn install(loc: &Location, opts: &TargetInstallOptions, home: &PathBuf) -> TargetResult {
    let mut out = Outcome::new(opts.dry_run);

    let kiro_dir = match loc {
        Location::Global => home.join(".kiro"),
        Location::Folder(folder) => folder.join(".kiro"),
    };

    // 1. MCP entry in ~/.kiro/settings/mcp.json or <folder>/.kiro/settings/mcp.json
    let mcp_path = kiro_dir.join("settings").join("mcp.json");
    upsert_mcp_server(
        &mut out,
        &mcp_path,
        "telemaco",
        mcp_entry(&opts.binary_path, opts.stealth, opts.auto_allow),
    );

    // 2. Hook in ~/.kiro/hooks/telemaco.json or <folder>/.kiro/hooks/telemaco.json
    let hooks_path = kiro_dir.join("hooks").join("telemaco.json");
    let hook_cmd = prompt_hook_command(&opts.binary_path);
    // Kiro passes the prompt in `USER_PROMPT` rather than in the stdin payload
    // (kiro.dev/docs/hooks/types, Prompt Submit), which `prompt-hook` reads as
    // a fallback. Its stdout on exit 0 "is added to the agent's context"
    // (kiro.dev/docs/hooks/actions), so plain text is the right format here.
    // `UserPromptSubmit` is the event name Kiro documents for the prompt
    // trigger (kiro.dev/docs/hooks/actions); "PromptSubmit" is the label in the
    // UI, not the identifier, so the hook never fired.
    let desired_hook = json!({
        "name": "Telemaco Prompt Hook",
        "trigger": "UserPromptSubmit",
        "action": {
            "type": "command",
            "command": hook_cmd
        }
    });

    // Kiro loads hook files from `.kiro/hooks/` in a project root, and
    // documents no user-level hooks directory (kiro.dev/docs/hooks): a global
    // install gets the directive through the steering file instead.
    if loc.is_global() {
        out.note(
            "Kiro reads hooks from a project's .kiro/hooks/ only, so no global prompt hook \
             is installed; the steering file covers every project.",
        );
    } else if !hooks_path.exists() {
        let content = json!({ "version": "v1", "hooks": [desired_hook] });
        out.write_json(&hooks_path, &content, Action::Created);
    } else if let Some(mut cur_json) = out.load_json(&hooks_path) {
        match cur_json.get_mut("hooks").and_then(|h| h.as_array_mut()) {
            Some(hooks_arr) => {
                let mut changed = false;
                let existing = hooks_arr
                    .iter_mut()
                    .find(|h| hook_command_of(h).map_or(false, is_telemaco_hook_command));
                match existing {
                    Some(entry) => {
                        if hook_command_of(entry) != Some(hook_cmd.as_str()) {
                            entry["action"]["command"] = json!(hook_cmd);
                            changed = true;
                        }
                        // Repair the trigger an older Telemaco wrote.
                        if entry.get("trigger").and_then(|t| t.as_str()) != Some("UserPromptSubmit") {
                            entry["trigger"] = json!("UserPromptSubmit");
                            changed = true;
                        }
                    }
                    None => {
                        hooks_arr.push(desired_hook);
                        changed = true;
                    }
                }
                if changed {
                    out.write_json(&hooks_path, &cur_json, Action::Updated);
                } else {
                    out.push(FileResult { path: hooks_path, action: Action::Unchanged });
                }
            }
            None => {
                let content = json!({ "version": "v1", "hooks": [desired_hook] });
                out.write_json(&hooks_path, &content, Action::Updated);
            }
        }
    }

    // 3. Instructions: steering file or AGENTS.md
    let instructions_path = match loc {
        Location::Global => kiro_dir.join("steering").join("telemaco.md"),
        Location::Folder(folder) => {
            if folder.join(".kiro").join("steering").exists() {
                folder.join(".kiro").join("steering").join("telemaco.md")
            } else {
                folder.join("AGENTS.md")
            }
        }
    };
    update_instructions(&mut out, &instructions_path, opts.stealth);

    out.finish(TargetId::Kiro)
}

pub fn uninstall(loc: &Location, home: &PathBuf, dry_run: bool) -> TargetResult {
    let mut out = Outcome::new(dry_run);

    let kiro_dir = match loc {
        Location::Global => home.join(".kiro"),
        Location::Folder(folder) => folder.join(".kiro"),
    };

    // 1. MCP
    remove_mcp_server(&mut out, &kiro_dir.join("settings").join("mcp.json"), "telemaco");

    // 2. Hooks
    let hooks_path = kiro_dir.join("hooks").join("telemaco.json");
    if hooks_path.exists() {
        if let Some(mut cur_json) = out.load_json(&hooks_path) {
            let mut modified = false;
            let mut now_empty = false;
            if let Some(hooks_arr) = cur_json.get_mut("hooks").and_then(|h| h.as_array_mut()) {
                let prev_len = hooks_arr.len();
                hooks_arr.retain(|h| !hook_command_of(h).map_or(false, is_telemaco_hook_command));
                if hooks_arr.len() != prev_len {
                    modified = true;
                }
                now_empty = hooks_arr.is_empty();
            }
            if now_empty {
                out.remove_file(&hooks_path);
            } else if modified {
                out.write_json_or_remove(&hooks_path, &cur_json, Action::Updated);
            }
        }
    }

    // 3. Instructions
    let instructions_paths = match loc {
        Location::Global => vec![kiro_dir.join("steering").join("telemaco.md")],
        Location::Folder(folder) => vec![
            folder.join(".kiro").join("steering").join("telemaco.md"),
            folder.join("AGENTS.md"),
        ],
    };
    for p in instructions_paths {
        remove_instructions(&mut out, &p);
    }

    out.finish(TargetId::Kiro)
}
