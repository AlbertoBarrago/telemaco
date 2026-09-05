use std::path::PathBuf;

use super::common::*;

/// Gemini CLI's user-level directory.
///
/// `~/.gemini` normally, but `GEMINI_CLI_HOME` moves the root the CLI creates
/// that `.gemini` folder inside ("Specifies the root directory for Gemini
/// CLI's user-level configuration and storage [...] The CLI will create a
/// `.gemini` folder inside this directory",
/// google-gemini/gemini-cli docs/reference/configuration.md). With it set,
/// everything written under `~/.gemini` is read by nothing.
fn gemini_dir(home: &PathBuf) -> PathBuf {
    home_env_var("GEMINI_CLI_HOME")
        .unwrap_or_else(|| home.clone())
        .join(".gemini")
}

fn config_dir_for(loc: &Location, home: &PathBuf) -> PathBuf {
    match loc {
        Location::Global => gemini_dir(home),
        Location::Folder(folder) => folder.join(".gemini"),
    }
}

/// The context file names Gemini actually loads.
///
/// `GEMINI.md` is only the default: `context.fileName` replaces it, and takes
/// either a name or a list of names that are all loaded and concatenated
/// (docs/cli/gemini-md.md, "Customize the context file name"). A project's
/// `.gemini/settings.json` wins over the user's. Written to the wrong name,
/// the block is loaded by nothing.
fn context_file_names(loc: &Location, home: &PathBuf) -> Vec<String> {
    let mut sources = Vec::new();
    if let Location::Folder(folder) = loc {
        sources.push(folder.join(".gemini").join("settings.json"));
    }
    sources.push(gemini_dir(home).join("settings.json"));

    configured_context_file_names(&sources).unwrap_or_else(|| vec!["GEMINI.md".to_string()])
}

/// The one file an install writes: the first name Gemini would load.
fn instructions_path(loc: &Location, home: &PathBuf) -> PathBuf {
    let name = context_file_names(loc, home)
        .into_iter()
        .next()
        .unwrap_or_else(|| "GEMINI.md".to_string());
    match loc {
        Location::Global => gemini_dir(home).join(name),
        Location::Folder(folder) => folder.join(name),
    }
}

/// Every context file a previous install may have written into.
fn all_instructions_paths(loc: &Location, home: &PathBuf) -> Vec<PathBuf> {
    let mut names = context_file_names(loc, home);
    if !names.iter().any(|n| n == "GEMINI.md") {
        names.push("GEMINI.md".to_string());
    }
    names
        .into_iter()
        .map(|name| match loc {
            Location::Global => gemini_dir(home).join(name),
            Location::Folder(folder) => folder.join(name),
        })
        .collect()
}

pub fn detect(loc: &Location, home: Option<&PathBuf>) -> DetectionResult {
    let h = home.cloned().unwrap_or_else(|| PathBuf::from("."));
    let config_dir = config_dir_for(loc, &h);
    let settings_path = config_dir.join("settings.json");
    let gemini_md = instructions_path(loc, &h);
    let installed = config_dir.exists() || settings_path.exists() || gemini_md.exists();
    let mut already_configured = false;
    if settings_path.exists() {
        let json = read_json_file(&settings_path);
        if let Some(servers) = json.get("mcpServers").and_then(|v| v.as_object()) {
            already_configured = servers.contains_key("telemaco");
        }
        if !already_configured {
            // Not "a BeforeAgent hook exists": the user's own hooks live there
            // too, and any one of them used to mark us as installed.
            already_configured = json
                .get("hooks")
                .and_then(|h| h.get("BeforeAgent"))
                .map_or(false, |v| text_has_telemaco_hook(&v.to_string()));
        }
    }
    let hint = if already_configured {
        "already configured".to_string()
    } else if installed {
        if loc.is_global() {
            "detected".to_string()
        } else {
            let mut markers: Vec<String> = Vec::new();
            if config_dir.exists() { markers.push(".gemini/".to_string()); }
            if gemini_md.exists() {
                if let Some(name) = gemini_md.file_name() {
                    markers.push(name.to_string_lossy().to_string());
                }
            }
            if markers.is_empty() { "detected".to_string() } else { markers.join(", ") }
        }
    } else {
        String::new()
    };
    DetectionResult {
        installed,
        already_configured,
        config_path: Some(settings_path),
        hint,
    }
}

pub fn install(loc: &Location, opts: &TargetInstallOptions, home: &PathBuf) -> TargetResult {
    let mut out = Outcome::new(opts.dry_run);

    let config_dir = config_dir_for(loc, home);
    // The MCP entry and the hook share one file, so they share one write:
    // upserting each in turn reported settings.json twice and rewrote it twice.
    let settings_path = config_dir.join("settings.json");
    let existed = settings_path.exists();
    if let Some(mut settings_json) = out.load_json(&settings_path) {
        let mut modified = match upsert_server_entry(
            &mut settings_json,
            "mcpServers",
            "telemaco",
            stdio_mcp_entry(&opts.binary_path, opts.stealth),
        ) {
            Ok(changed) => changed,
            Err(why) => {
                out.note(format!("{}: {}", settings_path.display(), why));
                false
            }
        };

        // Gemini CLI's prompt event is `BeforeAgent`, and it parses a command
        // hook's stdout as JSON: "your script must not print any plain text to
        // stdout other than the final JSON"
        // (google-gemini/gemini-cli docs/hooks/reference.md).
        if add_prompt_hook_for_event(
            &mut settings_json,
            "BeforeAgent",
            &prompt_hook_command_json(&opts.binary_path),
        ) {
            modified = true;
        }

        if modified {
            let action = if existed { Action::Updated } else { Action::Created };
            out.write_json(&settings_path, &settings_json, action);
        } else {
            out.push(FileResult { path: settings_path.clone(), action: Action::Unchanged });
        }
    }

    update_instructions(&mut out, &instructions_path(loc, home), opts.stealth);

    // Project hooks are fingerprinted: "If a hook's name or command changes
    // [...] it is treated as a new, untrusted hook and you will be warned
    // before it executes" (docs/hooks/index.md).
    if loc.folder().is_some() {
        out.note(
            "Gemini CLI warns you before running a project hook it has not seen before, and \
             again whenever its command changes, so the prompt hook starts working after you \
             accept it.",
        );
    }

    out.finish(TargetId::Gemini)
}

pub fn uninstall(loc: &Location, home: &PathBuf, dry_run: bool) -> TargetResult {
    let mut out = Outcome::new(dry_run);

    let settings_path = config_dir_for(loc, home).join("settings.json");
    if settings_path.exists() {
        if let Some(mut settings_json) = out.load_json(&settings_path) {
            let hook = remove_prompt_hook_for_event(&mut settings_json, "BeforeAgent");
            let server = settings_json
                .get_mut("mcpServers")
                .and_then(|v| v.as_object_mut())
                .map_or(false, |s| s.remove("telemaco").is_some());
            if hook || server {
                out.write_json_or_remove(&settings_path, &settings_json, Action::Updated);
            }
        }
    }

    // Both the configured name and the default: a settings change between
    // install and uninstall must not strand the block in the old file.
    for path in all_instructions_paths(loc, home) {
        remove_instructions(&mut out, &path);
    }

    out.finish(TargetId::Gemini)
}
