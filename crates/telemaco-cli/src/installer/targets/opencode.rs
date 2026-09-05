use std::path::PathBuf;
use serde_json::json;

use super::common::*;

fn config_path(loc: &Location, home: &PathBuf) -> PathBuf {
    let dir = match loc {
        Location::Global => home.join(".config").join("opencode"),
        Location::Folder(folder) => folder.clone(),
    };
    let jsonc_path = dir.join("opencode.jsonc");
    if jsonc_path.exists() {
        jsonc_path
    } else {
        dir.join("opencode.json")
    }
}

pub fn detect(loc: &Location, home: Option<&PathBuf>) -> DetectionResult {
    let h = home.cloned().unwrap_or_else(|| PathBuf::from("."));
    let chosen_path = config_path(loc, &h);
    let installed = match loc {
        Location::Global => h.join(".config").join("opencode").exists() || chosen_path.exists(),
        Location::Folder(folder) => {
            // Not `OPENCODE.md`: OpenCode's instructions live in `AGENTS.md`,
            // with `CLAUDE.md` as a fallback, and no file by that name appears
            // anywhere in its docs (opencode.ai/docs/rules).
            folder.join("opencode.json").exists()
                || folder.join("opencode.jsonc").exists()
                || folder.join(".opencode").exists()
        }
    };
    let mut already_configured = false;
    if chosen_path.exists() {
        let json = read_json_file(&chosen_path);
        if let Some(mcp) = json.get("mcp").and_then(|v| v.as_object()) {
            already_configured = mcp.contains_key("telemaco");
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
                if folder.join("opencode.json").exists() { markers.push("opencode.json"); }
                if folder.join("opencode.jsonc").exists() { markers.push("opencode.jsonc"); }
                if folder.join(".opencode").exists() { markers.push(".opencode/"); }
            }
            if markers.is_empty() { "detected".to_string() } else { markers.join(", ") }
        }
    } else {
        String::new()
    };
    DetectionResult {
        installed,
        already_configured,
        config_path: Some(chosen_path),
        hint,
    }
}

/// OpenCode takes the binary and its arguments as one command array.
pub fn mcp_entry(binary_path: &str, stealth: bool) -> serde_json::Value {
    let mut full_cmd = vec![binary_path.to_string()];
    full_cmd.extend(stdio_mcp_args(stealth));
    json!({
        "type": "local",
        "command": full_cmd,
        "enabled": true
    })
}

pub fn install(loc: &Location, opts: &TargetInstallOptions, home: &PathBuf) -> TargetResult {
    let mut out = Outcome::new(opts.dry_run);

    let chosen_path = config_path(loc, home);
    upsert_named_server(
        &mut out,
        &chosen_path,
        "mcp",
        "telemaco",
        mcp_entry(&opts.binary_path, opts.stealth),
    );

    let instructions_path = match loc {
        Location::Global => home.join(".config").join("opencode").join("AGENTS.md"),
        Location::Folder(folder) => folder.join("AGENTS.md"),
    };
    update_instructions(&mut out, &instructions_path, opts.stealth);

    out.finish(TargetId::OpenCode)
}

pub fn uninstall(loc: &Location, home: &PathBuf, dry_run: bool) -> TargetResult {
    let mut out = Outcome::new(dry_run);

    let paths = match loc {
        Location::Global => {
            let config_dir = home.join(".config").join("opencode");
            vec![config_dir.join("opencode.json"), config_dir.join("opencode.jsonc")]
        }
        Location::Folder(folder) => {
            vec![folder.join("opencode.json"), folder.join("opencode.jsonc")]
        }
    };
    for p in paths {
        remove_named_server(&mut out, &p, "mcp", "telemaco");
    }

    let instructions_path = match loc {
        Location::Global => home.join(".config").join("opencode").join("AGENTS.md"),
        Location::Folder(folder) => folder.join("AGENTS.md"),
    };
    remove_instructions(&mut out, &instructions_path);

    out.finish(TargetId::OpenCode)
}
