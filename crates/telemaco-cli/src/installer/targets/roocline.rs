use std::path::{Path, PathBuf};
use serde_json::json;

use super::common::*;

/// Project-level MCP config.
///
/// Roo Code reads `.roo/mcp.json` at the project root (docs: "Project-level
/// Configuration: Defined in a `.roo/mcp.json` file within your project's root
/// directory"). We used to write `.vscode/mcp.json`, which is VS Code's own
/// native MCP file and is read by neither Roo Code nor Cline.
fn project_mcp_path(folder: &Path) -> PathBuf {
    folder.join(".roo").join("mcp.json")
}

/// VS Code's per-user directory, the parent of `globalStorage`.
fn vscode_user_dir(home: &PathBuf) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home.join("Library/Application Support/Code/User")
    }
    #[cfg(not(target_os = "macos"))]
    {
        home.join(".config/Code/User")
    }
}

fn get_base_storage(home: &PathBuf) -> PathBuf {
    vscode_user_dir(home).join("globalStorage")
}

/// Roo Code's storage base.
///
/// Normally the extension's own `globalStorage` directory, but the
/// `roo-cline.customStoragePath` VS Code setting replaces it wholesale
/// (`getStorageBasePath`, Roo-Code `src/utils/storage.ts`). With that setting
/// pointing elsewhere, everything written under `globalStorage` is read by
/// nothing, the same trap `CODEX_HOME` and `HERMES_HOME` were.
fn roo_storage_base(home: &PathBuf) -> PathBuf {
    let settings = vscode_user_dir(home).join("settings.json");
    if settings.exists() {
        if let Some(custom) = read_json_file(&settings)
            .get("roo-cline.customStoragePath")
            .and_then(|v| v.as_str())
        {
            if !custom.trim().is_empty() {
                return PathBuf::from(custom.trim());
            }
        }
    }
    get_base_storage(home).join("rooveterinaryinc.roo-cline")
}

/// Cline's own configuration root, shared by the CLI, the SDK and Kanban
/// (docs.cline.bot/getting-started/config).
fn cline_home(home: &PathBuf) -> PathBuf {
    home.join(".cline")
}

/// The MCP files read at the user level, each with the directory that proves
/// its product is actually installed.
///
/// The marker is never a directory this installer creates: `~/.cline/rules/`
/// is ours, `~/.cline/data/` is Cline's, and guarding on the wrong one makes
/// the second install differ from the first.
///
/// - Roo Code: `<storage base>/settings/mcp_settings.json`
///   (`GlobalFileNames.mcpSettings`, Roo-Code `src/shared/globalFileNames.ts`).
///   Not `cline_mcp_settings.json`: that is the name Roo inherited from the
///   fork and stopped reading.
/// - Cline, VS Code extension: `<globalStorage>/saoudrizwan.claude-dev/settings/cline_mcp_settings.json`.
/// - Cline, CLI and SDK: `~/.cline/data/settings/cline_mcp_settings.json`.
///   The MCP page still documents `~/.cline/mcp.json`; the shipped CLI
///   (`cline mcp install`) writes the path above and reads only that one.
fn global_mcp_targets(home: &PathBuf) -> Vec<(PathBuf, PathBuf)> {
    let roo_base = roo_storage_base(home);
    let cline_extension = get_base_storage(home).join("saoudrizwan.claude-dev");
    let cline_root = cline_home(home);
    vec![
        (roo_base.join("settings").join("mcp_settings.json"), roo_base),
        (
            cline_extension.join("settings").join("cline_mcp_settings.json"),
            cline_extension,
        ),
        (
            cline_root
                .join("data")
                .join("settings")
                .join("cline_mcp_settings.json"),
            cline_root.join("data"),
        ),
    ]
}

/// MCP files earlier versions wrote, or that the docs still point at, cleaned
/// up on the way out.
fn legacy_global_mcp_paths(home: &PathBuf) -> Vec<PathBuf> {
    vec![
        roo_storage_base(home)
            .join("settings")
            .join("cline_mcp_settings.json"),
        cline_home(home).join("mcp.json"),
    ]
}

/// Rules files, one per product: Roo Code and Cline read different paths, and
/// neither of them reads the `~/.clinerules` we used to write.
///
/// Roo Code: `.roo/rules/` in a workspace, `~/.roo/rules/` globally
/// (docs.roocode.com/features/custom-instructions).
///
/// Cline: `.clinerules/` in a workspace, and globally both `~/.cline/rules/`
/// (CLI and SDK) and `~/Documents/Cline/Rules` (the VS Code extension's
/// documented directory, also read by the CLI). Cline keys a rule by its file
/// stem and loads the first one it finds, so the same `telemaco.md` in both
/// directories is one rule, not two.
fn instructions_paths(loc: &Location, home: &PathBuf) -> Vec<PathBuf> {
    match loc {
        Location::Global => vec![
            home.join(".roo").join("rules").join("telemaco.md"),
            cline_home(home).join("rules").join("telemaco.md"),
            home.join("Documents")
                .join("Cline")
                .join("Rules")
                .join("telemaco.md"),
        ],
        Location::Folder(folder) => vec![
            folder.join(".roo").join("rules").join("telemaco.md"),
            cline_workspace_rules(folder),
        ],
    }
}

/// `.clinerules` is a directory of rule files, but plenty of projects still
/// carry the older single file. Creating the directory on top of it would fail,
/// so an existing file is updated where it is.
fn cline_workspace_rules(folder: &Path) -> PathBuf {
    let base = folder.join(".clinerules");
    if base.is_file() {
        base
    } else {
        base.join("telemaco.md")
    }
}

/// Rules paths earlier versions wrote, cleaned up on the way out.
fn legacy_instructions(loc: &Location, home: &PathBuf) -> Vec<PathBuf> {
    match loc {
        Location::Global => vec![home.join(".clinerules")],
        // The workspace file is already the target of `cline_workspace_rules`.
        Location::Folder(_) => Vec::new(),
    }
}

pub fn detect(loc: &Location, home: Option<&PathBuf>) -> DetectionResult {
    let h = home.cloned().unwrap_or_else(|| PathBuf::from("."));
    let (chosen, installed) = match loc {
        Location::Global => {
            let targets = global_mcp_targets(&h);
            let chosen = targets
                .iter()
                .map(|(p, _)| p)
                .find(|p| p.exists())
                .cloned()
                .or_else(|| targets.first().map(|(p, _)| p.clone()));
            // Not `|| globalStorage.exists()`: every VS Code user has that
            // directory, and it says nothing about these extensions. The
            // markers are each product's own directory, so a Cline CLI that
            // has never had an MCP server still counts as installed.
            let installed = targets
                .iter()
                .any(|(path, marker)| path.exists() || marker.exists())
                || legacy_global_mcp_paths(&h).iter().any(|p| p.exists());
            (chosen, installed)
        }
        Location::Folder(folder) => {
            let mcp_path = project_mcp_path(folder);
            let installed = folder.join(".clinerules").exists()
                || folder.join(".roomodes").exists()
                || folder.join(".roo").exists()
                || folder.join(".cline").exists()
                || mcp_path.exists();
            (Some(mcp_path), installed)
        }
    };
    let mut already_configured = false;
    if let Some(ref p) = chosen {
        if p.exists() {
            let json = read_json_file(p);
            if let Some(servers) = json.get("mcpServers").and_then(|v| v.as_object()) {
                already_configured = servers.contains_key("telemaco");
            }
        }
    }
    if !already_configured {
        // The rules files are the other half of the install, and for a folder
        // without an MCP file they are the whole of it.
        already_configured = instructions_paths(loc, &h).iter().any(|p| {
            std::fs::read_to_string(p).map_or(false, |c| c.contains("<!-- TELEMACO_START -->"))
        });
    }
    let hint = if already_configured {
        "already configured".to_string()
    } else if installed {
        if loc.is_global() {
            "detected".to_string()
        } else {
            let mut markers = Vec::new();
            if let Location::Folder(folder) = loc {
                if folder.join(".clinerules").exists() { markers.push(".clinerules"); }
                if folder.join(".roomodes").exists() { markers.push(".roomodes"); }
                if folder.join(".roo").exists() { markers.push(".roo/"); }
                if folder.join(".cline").exists() { markers.push(".cline/"); }
            }
            if markers.is_empty() { "detected".to_string() } else { markers.join(", ") }
        }
    } else {
        String::new()
    };
    DetectionResult {
        installed,
        already_configured,
        config_path: chosen,
        hint,
    }
}

/// Roo Code and Cline carry their own enable/approve fields.
pub fn mcp_entry(binary_path: &str, stealth: bool) -> serde_json::Value {
    json!({
        "command": binary_path,
        "args": stdio_mcp_args(stealth),
        "disabled": false,
        "autoApprove": []
    })
}

pub fn install(loc: &Location, opts: &TargetInstallOptions, home: &PathBuf) -> TargetResult {
    let mut out = Outcome::new(opts.dry_run);

    let target_entry = mcp_entry(&opts.binary_path, opts.stealth);

    match loc {
        Location::Global => {
            for (path, marker) in global_mcp_targets(home) {
                if marker.exists() || path.exists() {
                    upsert_mcp_server(&mut out, &path, "telemaco", target_entry.clone());
                }
            }
        }
        // Only Roo Code reads a project MCP file. Cline's `.cline/` holds
        // rules, hooks and plugins; its servers come from the user-level file
        // whatever directory the session runs in.
        Location::Folder(folder) => {
            upsert_mcp_server(&mut out, &project_mcp_path(folder), "telemaco", target_entry);
        }
    }

    for path in instructions_paths(loc, home) {
        update_instructions(&mut out, &path, opts.stealth);
    }

    out.finish(TargetId::RooCline)
}

pub fn uninstall(loc: &Location, home: &PathBuf, dry_run: bool) -> TargetResult {
    let mut out = Outcome::new(dry_run);

    let paths = match loc {
        Location::Global => global_mcp_targets(home)
            .into_iter()
            .map(|(path, _)| path)
            .chain(legacy_global_mcp_paths(home))
            .collect(),
        Location::Folder(folder) => vec![
            project_mcp_path(folder),
            // Earlier builds wrote here; clean it up on the way out.
            folder.join(".vscode").join("mcp.json"),
        ],
    };

    for path in paths {
        remove_mcp_server(&mut out, &path, "telemaco");
    }

    for path in instructions_paths(loc, home)
        .into_iter()
        .chain(legacy_instructions(loc, home))
    {
        remove_instructions(&mut out, &path);
    }

    out.finish(TargetId::RooCline)
}
