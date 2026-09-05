use std::path::PathBuf;
use serde_json::{json, Value};

use super::common::*;

/// Antigravity's customization directory.
///
/// Global is `~/.gemini/config`, workspace is `.agents` in the project
/// (antigravity.google/docs/mcp, /docs/hooks). We used to write
/// `<folder>/.gemini/config` for a project and to fall back to
/// `~/.gemini/antigravity` when the global directory did not exist yet: the
/// first is not read at all, and the second holds OAuth tokens and session
/// state, not configuration.
fn config_dir(loc: &Location, home: &PathBuf) -> PathBuf {
    match loc {
        Location::Global => home.join(".gemini").join("config"),
        Location::Folder(folder) => folder.join(".agents"),
    }
}

/// Where earlier versions wrote, cleaned up on the way out.
fn legacy_paths(loc: &Location, home: &PathBuf) -> Vec<PathBuf> {
    match loc {
        Location::Global => vec![home.join(".gemini").join("antigravity").join("mcp_config.json")],
        Location::Folder(folder) => vec![folder
            .join(".gemini")
            .join("config")
            .join("mcp_config.json")],
    }
}

pub fn detect(loc: &Location, home: Option<&PathBuf>) -> DetectionResult {
    let h = home.cloned().unwrap_or_else(|| PathBuf::from("."));
    let mcp_path = config_dir(loc, &h).join("mcp_config.json");
    let installed = match loc {
        Location::Global => {
            // `antigravity` is the app's own directory, `antigravity-cli` the
            // CLI's (antigravity.google/docs/cli/settings keeps its settings
            // in `~/.gemini/antigravity-cli/`). A machine with only the CLI
            // has the second and not the first.
            h.join(".gemini").join("antigravity").exists()
                || h.join(".gemini").join("antigravity-cli").exists()
                || h.join(".gemini").join("config").exists()
                || mcp_path.exists()
        }
        Location::Folder(folder) => {
            // Not the bare `.agents/`: Factory Droid documents it as a shared
            // agent-folder convention, so any project using it came back as an
            // Antigravity install. Only the files Antigravity itself owns count.
            let agents = folder.join(".agents");
            agents.join("hooks.json").exists()
                || agents.join("rules").exists()
                || folder.join(".antigravity").exists()
                || folder.join(".gemini").join("antigravity").exists()
                || mcp_path.exists()
        }
    };
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
            if let Location::Folder(folder) = loc {
                if folder.join(".agents").join("mcp_config.json").exists()
                    || folder.join(".agents").join("hooks.json").exists()
                    || folder.join(".agents").join("rules").exists()
                {
                    markers.push(".agents/");
                }
                if folder.join(".antigravity").exists() { markers.push(".antigravity/"); }
                if folder.join(".gemini").join("antigravity").exists() { markers.push(".gemini/antigravity/"); }
            }
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

/// Instruction files, one per surface that reads one.
///
/// Global rules live in `~/.gemini/GEMINI.md` for both the IDE and the CLI
/// (antigravity.google/docs/rules-workflows, /docs/cli/gcli-migration).
///
/// A project needs two files, because the two surfaces read different ones.
/// The IDE takes workspace rules from `.agents/rules`. The CLI does not: it
/// "continues to parse and enforce rule constraints defined inside your active
/// directory's `GEMINI.md` and `AGENTS.md` files"
/// (antigravity.google/docs/cli/gcli-migration). Writing only the rules file
/// left the CLI with no directive at all.
fn instructions_paths(loc: &Location, home: &PathBuf) -> Vec<PathBuf> {
    match loc {
        Location::Global => vec![home.join(".gemini").join("GEMINI.md")],
        Location::Folder(folder) => vec![
            folder.join(".agents").join("rules").join("telemaco.md"),
            folder.join("AGENTS.md"),
        ],
    }
}

pub fn install(loc: &Location, opts: &TargetInstallOptions, home: &PathBuf) -> TargetResult {
    let mut out = Outcome::new(opts.dry_run);

    let dir = config_dir(loc, home);
    upsert_mcp_server(
        &mut out,
        &dir.join("mcp_config.json"),
        "telemaco",
        stdio_mcp_entry(&opts.binary_path, opts.stealth),
    );

    // 2. Hooks live in hooks.json in the same customization directory, keyed by
    //    a hook name of our choosing.
    let hooks_path = dir.join("hooks.json");
    let hooks_existed = hooks_path.exists();
    if let Some(mut hooks_json) = out.load_json(&hooks_path) {
        let pre_invocation = json!([
            {
                "command": prompt_hook_command(&opts.binary_path),
                "type": "command",
                "timeout": 10
            }
        ]);
        // Only `PreInvocation` is ours. Replacing the whole entry dropped
        // anything else the user had put under our hook name, `enabled: false`
        // included, so a reinstall silently switched a disabled hook back on.
        let mut merged = match hooks_json.get("telemaco") {
            Some(Value::Object(existing)) => existing.clone(),
            _ => serde_json::Map::new(),
        };
        merged.insert("PreInvocation".to_string(), pre_invocation);
        let merged = Value::Object(merged);

        if hooks_json.get("telemaco").map_or(false, |cur| json_deep_equal(cur, &merged)) {
            out.push(FileResult { path: hooks_path, action: Action::Unchanged });
        } else {
            hooks_json
                .as_object_mut()
                .expect("load_json guarantees an object")
                .insert("telemaco".to_string(), merged);
            let action = if hooks_existed { Action::Updated } else { Action::Created };
            out.write_json(&hooks_path, &hooks_json, action);
        }
    }

    // 3. Instructions.
    //
    // No frontmatter: Antigravity documents four activation modes for a
    // workspace rule but not the file syntax that selects one
    // (antigravity.google/docs/rules-workflows), so inventing a key would be
    // guesswork. The file is written plain and the user is told where the
    // switch lives.
    for path in instructions_paths(loc, home) {
        update_instructions(&mut out, &path, opts.stealth);
    }
    if loc.folder().is_some() {
        out.note(
            "Antigravity sets a workspace rule's activation mode in the Rules panel, \
             not in the file: open it and set the Telemaco rule to Always On. The CLI \
             reads the project's AGENTS.md instead, which needs no switch.",
        );
    }

    out.finish(TargetId::Antigravity)
}

pub fn uninstall(loc: &Location, home: &PathBuf, dry_run: bool) -> TargetResult {
    let mut out = Outcome::new(dry_run);

    let dir = config_dir(loc, home);
    remove_mcp_server(&mut out, &dir.join("mcp_config.json"), "telemaco");
    for legacy in legacy_paths(loc, home) {
        remove_mcp_server(&mut out, &legacy, "telemaco");
    }

    let hooks_path = dir.join("hooks.json");
    if hooks_path.exists() {
        if let Some(mut json) = out.load_json(&hooks_path) {
            let removed = json
                .as_object_mut()
                .expect("load_json guarantees an object")
                .remove("telemaco")
                .is_some();
            if removed {
                out.write_json_or_remove(&hooks_path, &json, Action::Removed);
            }
        }
    }

    // Global installs write into ~/.gemini/GEMINI.md, which Gemini CLI shares.
    // Removing a block that is not there is a no-op, so doing it from both
    // targets is safe.
    for path in instructions_paths(loc, home) {
        remove_instructions(&mut out, &path);
    }

    out.finish(TargetId::Antigravity)
}
