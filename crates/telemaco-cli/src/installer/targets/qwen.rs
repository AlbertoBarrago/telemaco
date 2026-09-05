use std::path::PathBuf;

use super::common::*;

fn config_dir_for(loc: &Location, home: &PathBuf) -> PathBuf {
    match loc {
        Location::Global => home.join(".qwen"),
        Location::Folder(folder) => folder.join(".qwen"),
    }
}

/// The context file names Qwen was told to load, if the default was replaced.
///
/// `QWEN.md` is the default, "configurable via the `context.fileName` setting"
/// (QwenLM/qwen-code docs/users/configuration/settings.md). A project's
/// `.qwen/settings.json` overrides the user's.
fn configured_names(loc: &Location, home: &PathBuf) -> Option<Vec<String>> {
    let mut sources = Vec::new();
    if let Location::Folder(folder) = loc {
        sources.push(folder.join(".qwen").join("settings.json"));
    }
    sources.push(home.join(".qwen").join("settings.json"));
    configured_context_file_names(&sources)
}

/// The context file to write.
///
/// With a configured name, that name and nothing else: the default is not
/// loaded any more. Without one, an existing `QWEN.md` if the project has one,
/// otherwise `AGENTS.md`, which Qwen reads as well ("If your repository already
/// has an `AGENTS.md` file for other AI tools, Qwen reads that too",
/// docs/users/features/memory.md) and which the other agents share.
fn instructions_path(loc: &Location, home: &PathBuf) -> PathBuf {
    let configured = configured_names(loc, home);
    match loc {
        Location::Global => {
            let name = configured
                .and_then(|n| n.into_iter().next())
                .unwrap_or_else(|| "QWEN.md".to_string());
            home.join(".qwen").join(name)
        }
        Location::Folder(folder) => match configured {
            Some(names) => names
                .iter()
                .map(|n| folder.join(n))
                .find(|p| p.exists())
                .unwrap_or_else(|| folder.join(&names[0])),
            None => {
                if folder.join("QWEN.md").exists() {
                    folder.join("QWEN.md")
                } else {
                    folder.join("AGENTS.md")
                }
            }
        },
    }
}

/// Every context file a previous install may have written into.
fn all_instructions_paths(loc: &Location, home: &PathBuf) -> Vec<PathBuf> {
    let mut names = configured_names(loc, home).unwrap_or_default();
    for default in ["QWEN.md", "AGENTS.md"] {
        if !names.iter().any(|n| n == default) {
            names.push(default.to_string());
        }
    }
    match loc {
        // A global install never writes AGENTS.md: `~/.qwen/AGENTS.md` is not
        // a file Qwen reads.
        Location::Global => names
            .into_iter()
            .filter(|n| n != "AGENTS.md")
            .map(|n| home.join(".qwen").join(n))
            .collect(),
        Location::Folder(folder) => names.into_iter().map(|n| folder.join(n)).collect(),
    }
}

pub fn detect(loc: &Location, home: Option<&PathBuf>) -> DetectionResult {
    let h = home.cloned().unwrap_or_else(|| PathBuf::from("."));
    let qwen_dir = config_dir_for(loc, &h);
    let settings_path = qwen_dir.join("settings.json");
    let installed = match loc {
        Location::Global => qwen_dir.exists() || settings_path.exists(),
        Location::Folder(folder) => {
            qwen_dir.exists() || settings_path.exists() || folder.join("QWEN.md").exists()
        }
    };
    let mut already_configured = false;
    if settings_path.exists() {
        let json = read_json_file(&settings_path);
        if let Some(servers) = json.get("mcpServers").and_then(|v| v.as_object()) {
            already_configured = servers.contains_key("telemaco");
        }
        if !already_configured {
            // Our hook, not any UserPromptSubmit hook: the user's own live
            // in the same array.
            already_configured = json
                .get("hooks")
                .and_then(|hooks| hooks.get("UserPromptSubmit"))
                .map_or(false, |v| text_has_telemaco_hook(&v.to_string()));
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
                if folder.join(".qwen").exists() { markers.push(".qwen/"); }
                if folder.join("QWEN.md").exists() { markers.push("QWEN.md"); }
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
    let settings_path = config_dir.join("settings.json");
    let settings_existed = settings_path.exists();
    if let Some(mut settings_json) = out.load_json(&settings_path) {
        let target_entry = stdio_mcp_entry(&opts.binary_path, opts.stealth);

        // Through the shared helper, so the entry is merged rather than
        // replaced: this target hand-rolled the insert and so never got the
        // fix that keeps a user's `env` or `timeout` across a reinstall.
        let mut modified = match upsert_server_entry(
            &mut settings_json,
            "mcpServers",
            "telemaco",
            target_entry,
        ) {
            Ok(changed) => changed,
            Err(why) => {
                // Recorded, then carry on: the instructions file does not
                // depend on this key being usable.
                out.note(format!("{}: {}", settings_path.display(), why));
                false
            }
        };

        // Qwen parses a command hook's stdout as JSON and ignores anything
        // else, so the plain-text directive never reached the model
        // (QwenLM/qwen-code docs/users/features/hooks.md).
        if add_user_prompt_hook(&mut settings_json, &prompt_hook_command_json(&opts.binary_path)) {
            modified = true;
        }

        if modified {
            let action = if settings_existed { Action::Updated } else { Action::Created };
            out.write_json(&settings_path, &settings_json, action);
        } else {
            out.push(FileResult { path: settings_path, action: Action::Unchanged });
        }
    }

    update_instructions(&mut out, &instructions_path(loc, home), opts.stealth);

    // Folder trust is off by default, but once it is on an untrusted project
    // has its `.qwen/settings.json` ignored outright
    // (docs/users/configuration/trusted-folders.md).
    if loc.folder().is_some() {
        out.note(
            "With folder trust enabled, Qwen Code ignores a project's .qwen/settings.json until \
             you trust the folder, so the MCP entry and the prompt hook start working after that.",
        );
    }

    out.finish(TargetId::QwenCode)
}

pub fn uninstall(loc: &Location, home: &PathBuf, dry_run: bool) -> TargetResult {
    let mut out = Outcome::new(dry_run);

    let settings_path = config_dir_for(loc, home).join("settings.json");
    if settings_path.exists() {
        if let Some(mut json) = out.load_json(&settings_path) {
            let mut modified = false;
            if let Some(servers) = json.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
                if servers.remove("telemaco").is_some() {
                    modified = true;
                }
            }
            if remove_user_prompt_hook(&mut json) {
                modified = true;
            }
            if modified {
                out.write_json_or_remove(&settings_path, &json, Action::Updated);
            }
        }
    }

    for p in all_instructions_paths(loc, home) {
        remove_instructions(&mut out, &p);
    }

    out.finish(TargetId::QwenCode)
}
