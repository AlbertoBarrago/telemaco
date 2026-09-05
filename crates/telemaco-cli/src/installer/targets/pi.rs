use std::fs;
use std::path::PathBuf;

use super::common::*;

/// Pi's configuration directory: `~/.pi/agent` unless `PI_CODING_AGENT_DIR`
/// says otherwise, which is the variable the CLI itself reads
/// (nix-community.github.io/home-manager, programs.pi-coding-agent.configDir,
/// "matching the upstream pi CLI default"). With the variable set, everything
/// written under `~/.pi` is read by nothing, the same trap `CODEX_HOME` was.
fn agent_dir(home: &PathBuf) -> PathBuf {
    home_env_var("PI_CODING_AGENT_DIR").unwrap_or_else(|| home.join(".pi").join("agent"))
}

/// The context file Pi loads from a directory.
///
/// "Pi loads `AGENTS.md` (or `CLAUDE.md`) at startup from `~/.pi/agent/`,
/// parent directories and the current directory [...] If a directory contains
/// `AGENTS.override.md`, Pi loads it instead of `AGENTS.md` or `CLAUDE.md`
/// from that directory" (earendil-works/pi packages/coding-agent/README.md).
/// Written to `AGENTS.md` next to an override, the block is loaded by nothing.
fn instructions_path(loc: &Location, home: &PathBuf) -> PathBuf {
    match loc {
        Location::Global => agents_file_in(&agent_dir(home)),
        Location::Folder(folder) => agents_file_in(folder),
    }
}

/// Both names, for cleanup.
fn all_instructions_paths(loc: &Location, home: &PathBuf) -> Vec<PathBuf> {
    match loc {
        Location::Global => all_agents_files_in(&agent_dir(home)),
        Location::Folder(folder) => all_agents_files_in(folder),
    }
}

pub fn detect(loc: &Location, home: Option<&PathBuf>) -> DetectionResult {
    let h = home.cloned().unwrap_or_else(|| PathBuf::from("."));
    let instr_path = instructions_path(loc, &h);
    // Only the .pi directory identifies Pi. Looking for "pi" inside AGENTS.md
    // matched "pipeline", "API" and "expiry", so every repo with an AGENTS.md
    // came back as a Pi install.
    let installed = match loc {
        Location::Global => h.join(".pi").exists() || agent_dir(&h).exists(),
        Location::Folder(folder) => folder.join(".pi").exists(),
    };
    let mut already_configured = false;
    if instr_path.exists() {
        if let Ok(content) = fs::read_to_string(&instr_path) {
            already_configured = content.contains("<!-- TELEMACO_START -->");
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
                if folder.join(".pi").exists() { markers.push(".pi/"); }
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
    update_instructions(&mut out, &instructions_path(loc, home), opts.stealth);
    out.finish(TargetId::Pi)
}

pub fn uninstall(loc: &Location, home: &PathBuf, dry_run: bool) -> TargetResult {
    let mut out = Outcome::new(dry_run);
    for path in all_instructions_paths(loc, home) {
        remove_instructions(&mut out, &path);
    }
    out.finish(TargetId::Pi)
}
