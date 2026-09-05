use std::fs;
use std::path::PathBuf;

use super::common::*;

pub fn detect(loc: &Location, home: Option<&PathBuf>) -> DetectionResult {
    let (factory_dir, mcp_path, installed) = match loc {
        Location::Global => {
            let h = home.cloned().unwrap_or_else(|| PathBuf::from("."));
            let factory_dir = h.join(".factory");
            let mcp = factory_dir.join("mcp.json");
            let installed = factory_dir.exists() || mcp.exists();
            (factory_dir, mcp, installed)
        }
        Location::Folder(folder) => {
            let factory_dir = folder.join(".factory");
            let mcp = factory_dir.join("mcp.json");
            let installed = factory_dir.exists() || mcp.exists();
            (factory_dir, mcp, installed)
        }
    };
    let mut already_configured = false;
    if mcp_path.exists() {
        let json = read_json_file(&mcp_path);
        if let Some(servers) = json.get("mcpServers").and_then(|v| v.as_object()) {
            already_configured = servers.contains_key("telemaco");
        }
    }
    let hooks_path = factory_dir.join("hooks.json");
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
            if let Location::Folder(folder) = loc {
                if folder.join(".factory").exists() { markers.push(".factory/"); }
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

pub fn install(loc: &Location, opts: &TargetInstallOptions, home: &PathBuf) -> TargetResult {
    let mut out = Outcome::new(opts.dry_run);

    let config_dir = match loc {
        Location::Global => home.join(".factory"),
        Location::Folder(folder) => folder.join(".factory"),
    };
    let mcp_path = config_dir.join("mcp.json");
    upsert_mcp_server(
        &mut out,
        &mcp_path,
        "telemaco",
        stdio_typed_mcp_entry(&opts.binary_path, opts.stealth),
    );

    // Droid's standalone hooks.json is keyed directly by event name; the
    // `hooks` wrapper is only for settings.json, so the wrapped file we used to
    // write was loaded and ignored (docs.factory.ai/harness/hooks).
    let hooks_path = config_dir.join("hooks.json");
    let hooks_existed = hooks_path.exists();
    if let Some(mut hooks_json) = out.load_json(&hooks_path) {
        // Clear out the wrapper an older Telemaco left behind.
        let mut modified = remove_user_prompt_hook(&mut hooks_json);
        if modified {
            prune_installer_scaffolding(&mut hooks_json);
        }
        if add_user_prompt_hook_flat(&mut hooks_json, &prompt_hook_command(&opts.binary_path)) {
            modified = true;
        }
        if modified {
            let action = if hooks_existed { Action::Updated } else { Action::Created };
            out.write_json(&hooks_path, &hooks_json, action);
        }
    }

    let instructions_path = match loc {
        Location::Global => config_dir.join("AGENTS.md"),
        Location::Folder(folder) => folder.join("AGENTS.md"),
    };
    update_instructions(&mut out, &instructions_path, opts.stealth);

    out.finish(TargetId::Droid)
}

pub fn uninstall(loc: &Location, home: &PathBuf, dry_run: bool) -> TargetResult {
    let mut out = Outcome::new(dry_run);

    let config_dir = match loc {
        Location::Global => home.join(".factory"),
        Location::Folder(folder) => folder.join(".factory"),
    };
    remove_mcp_server(&mut out, &config_dir.join("mcp.json"), "telemaco");

    let hooks_path = config_dir.join("hooks.json");
    if hooks_path.exists() {
        if let Some(mut hooks_json) = out.load_json(&hooks_path) {
            // Both shapes: the flat one we write now, and the wrapped one
            // older versions wrote.
            let flat = remove_user_prompt_hook_flat(&mut hooks_json);
            let wrapped = remove_user_prompt_hook(&mut hooks_json);
            if flat || wrapped {
                out.write_json_or_remove(&hooks_path, &hooks_json, Action::Updated);
            }
        }
    }

    let instructions_path = match loc {
        Location::Global => config_dir.join("AGENTS.md"),
        Location::Folder(folder) => folder.join("AGENTS.md"),
    };
    remove_instructions(&mut out, &instructions_path);

    out.finish(TargetId::Droid)
}
