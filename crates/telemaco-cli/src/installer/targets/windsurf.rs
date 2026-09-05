use std::path::{Path, PathBuf};

use super::common::*;

/// Rules file for a project.
///
/// `.devin/rules/` is the preferred directory and `.windsurf/rules/` the
/// fallback; the single-file `.windsurfrules` at the root is legacy but "also
/// still read" (docs.windsurf.com/windsurf/cascade/memories). A rule file in
/// either directory declares its activation mode in frontmatter, while the
/// legacy root file is always on.
///
/// The test is the rules directory, never the bare `.devin/`: the MCP entry
/// creates that directory itself, so keying off it made the answer depend on
/// whether the install had already run - `--dry-run` named `.windsurfrules`
/// and the install that followed wrote somewhere else.
fn workspace_rules_path(folder: &Path) -> PathBuf {
    if folder.join(".windsurf").join("rules").exists() {
        folder.join(".windsurf").join("rules").join("telemaco.md")
    } else if !folder.join(".devin").join("rules").exists()
        && folder.join(".windsurfrules").exists()
    {
        folder.join(".windsurfrules")
    } else {
        folder.join(".devin").join("rules").join("telemaco.md")
    }
}

/// Every MCP config a Windsurf user's agent actually reads.
///
/// Cascade reads `~/.codeium/windsurf/mcp_config.json` and has no project file
/// at all, and that page says so itself: the MCP configuration there "applies
/// to the legacy Cascade agent only", while the Devin Local agent, the default
/// for new tabs, "configures MCP servers in the Devin CLI config files"
/// (docs.windsurf.com/windsurf/cascade/mcp). Those are
/// `~/.config/devin/mcp_config.json` and `.devin/mcp_config.json`, both keyed
/// by `mcpServers` (docs.devin.ai/cli/extensibility/mcp/configuration). A
/// project install used to write `.codeium/windsurf/mcp_config.json`, which
/// neither agent opens.
fn mcp_paths(loc: &Location, home: &PathBuf) -> Vec<PathBuf> {
    match loc {
        Location::Global => vec![
            home.join(".codeium").join("windsurf").join("mcp_config.json"),
            home.join(".config").join("devin").join("mcp_config.json"),
        ],
        Location::Folder(folder) => vec![folder.join(".devin").join("mcp_config.json")],
    }
}

/// MCP paths earlier versions wrote, taken back out on uninstall.
fn legacy_mcp_paths(loc: &Location) -> Vec<PathBuf> {
    match loc {
        Location::Global => Vec::new(),
        Location::Folder(folder) => vec![
            folder.join(".windsurf").join("mcp.json"),
            folder.join(".codeium").join("windsurf").join("mcp_config.json"),
        ],
    }
}

/// Every project path we may have written, for cleanup.
fn all_workspace_rules(folder: &Path) -> Vec<PathBuf> {
    vec![
        folder.join(".devin").join("rules").join("telemaco.md"),
        folder.join(".windsurf").join("rules").join("telemaco.md"),
        folder.join(".windsurfrules"),
    ]
}

/// Where the prompt hook goes, and whether that file wraps it.
///
/// The Devin CLI runs `UserPromptSubmit` hooks and injects whatever they print
/// as `hookSpecificOutput.additionalContext`, the same shape Claude Code and
/// Qwen use (docs.devin.ai/cli/extensibility/hooks/lifecycle-hooks). Cascade
/// has no hook of its own, which is why this target had none at all.
///
/// The file decides the shape: in `.devin/hooks.v1.json` "the hooks object is
/// the entire file", while every other location nests it under a `hooks` key
/// (docs.devin.ai/cli/extensibility/hooks/overview).
fn hooks_target(loc: &Location, home: &PathBuf) -> (PathBuf, bool) {
    match loc {
        Location::Global => (
            home.join(".config").join("devin").join("config.json"),
            true,
        ),
        Location::Folder(folder) => (folder.join(".devin").join("hooks.v1.json"), false),
    }
}

/// Global rule files, one per agent that reads one.
///
/// Cascade keeps its global rules in `~/.codeium/windsurf/memories/`. The Devin
/// CLI does not read that file: its user-level rules are `~/.devin/rules/*.md`
/// and `~/.devin/global_rules.md`, with the same `trigger` frontmatter a
/// project rule uses (docs.devin.ai/cli/extensibility/rules). A global install
/// that wrote only the Cascade file left the default agent with nothing.
fn global_rule_paths(home: &PathBuf) -> Vec<(PathBuf, Option<&'static str>)> {
    vec![
        (
            home.join(".codeium")
                .join("windsurf")
                .join("memories")
                .join("global_rules.md"),
            None,
        ),
        (
            home.join(".devin").join("rules").join("telemaco.md"),
            Some("trigger: always_on"),
        ),
    ]
}

pub fn detect(loc: &Location, home: Option<&PathBuf>) -> DetectionResult {
    let h = home.cloned().unwrap_or_else(|| PathBuf::from("."));
    let candidates: Vec<PathBuf> = mcp_paths(loc, &h)
        .into_iter()
        .chain(legacy_mcp_paths(loc))
        .collect();
    let installed = match loc {
        Location::Global => {
            h.join(".codeium").exists() || h.join(".config").join("devin").exists()
        }
        Location::Folder(folder) => {
            folder.join(".codeium").exists()
                || folder.join(".windsurf").exists()
                || folder.join(".devin").exists()
                || folder.join(".windsurfrules").exists()
        }
    };
    let has_us = |path: &PathBuf| {
        if !path.exists() {
            return false;
        }
        let json = read_json_file(path);
        json.get("mcpServers")
            .and_then(|v| v.as_object())
            .map_or(false, |s| s.contains_key("telemaco"))
    };
    let already_configured = candidates.iter().any(has_us);
    let mcp_path = candidates
        .iter()
        .find(|p| p.exists())
        .cloned()
        .unwrap_or_else(|| candidates[0].clone());
    let hint = if already_configured {
        "already configured".to_string()
    } else if installed {
        if loc.is_global() {
            "detected".to_string()
        } else {
            let mut markers = Vec::new();
            if let Location::Folder(folder) = loc {
                if folder.join(".devin").exists() { markers.push(".devin/"); }
                if folder.join(".windsurf").exists() { markers.push(".windsurf/"); }
                if folder.join(".codeium").exists() { markers.push(".codeium/"); }
                if folder.join(".windsurfrules").exists() { markers.push(".windsurfrules"); }
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

    for mcp_path in mcp_paths(loc, home) {
        upsert_mcp_server(
            &mut out,
            &mcp_path,
            "telemaco",
            stdio_mcp_entry(&opts.binary_path, opts.stealth),
        );
    }

    let (hooks_path, wrapped) = hooks_target(loc, home);
    let hooks_existed = hooks_path.exists();
    if let Some(mut hooks_json) = out.load_json(&hooks_path) {
        let cmd = prompt_hook_command_json(&opts.binary_path);
        let changed = if wrapped {
            add_user_prompt_hook(&mut hooks_json, &cmd)
        } else {
            add_user_prompt_hook_flat(&mut hooks_json, &cmd)
        };
        if changed {
            let action = if hooks_existed { Action::Updated } else { Action::Created };
            out.write_json(&hooks_path, &hooks_json, action);
        } else {
            out.push(FileResult { path: hooks_path, action: Action::Unchanged });
        }
    }

    match loc {
        Location::Global => {
            // Cascade's global rules file takes no frontmatter: it is always
            // on. The Devin one declares its trigger like any other rule.
            for (path, frontmatter) in global_rule_paths(home) {
                match frontmatter {
                    Some(fm) => update_rule_file(&mut out, &path, fm, opts.stealth),
                    None => update_instructions(&mut out, &path, opts.stealth),
                }
            }
        }
        Location::Folder(folder) => {
            let path = workspace_rules_path(folder);
            if path.file_name().map_or(false, |n| n == "telemaco.md") {
                update_rule_file(&mut out, &path, "trigger: always_on", opts.stealth);
            } else {
                update_instructions(&mut out, &path, opts.stealth);
            }
        }
    }

    out.finish(TargetId::Windsurf)
}

pub fn uninstall(loc: &Location, home: &PathBuf, dry_run: bool) -> TargetResult {
    let mut out = Outcome::new(dry_run);

    for mcp_path in mcp_paths(loc, home).into_iter().chain(legacy_mcp_paths(loc)) {
        remove_mcp_server(&mut out, &mcp_path, "telemaco");
    }

    let (hooks_path, wrapped) = hooks_target(loc, home);
    if hooks_path.exists() {
        if let Some(mut hooks_json) = out.load_json(&hooks_path) {
            let removed = if wrapped {
                remove_user_prompt_hook(&mut hooks_json)
            } else {
                remove_user_prompt_hook_flat(&mut hooks_json)
            };
            if removed {
                out.write_json_or_remove(&hooks_path, &hooks_json, Action::Updated);
            }
        }
    }

    match loc {
        Location::Global => {
            for (path, frontmatter) in global_rule_paths(home) {
                if frontmatter.is_some() {
                    // A rule file we created outright.
                    if path.exists() {
                        out.remove_file(&path);
                    }
                } else {
                    remove_instructions(&mut out, &path);
                }
            }
        }
        Location::Folder(folder) => {
            for path in all_workspace_rules(folder) {
                if !path.exists() {
                    continue;
                }
                if path.file_name().map_or(false, |n| n == "telemaco.md") {
                    // A rule file we created outright: taking only the block
                    // out would leave the frontmatter behind as a husk.
                    out.remove_file(&path);
                } else {
                    remove_instructions(&mut out, &path);
                }
            }
        }
    }

    out.finish(TargetId::Windsurf)
}
