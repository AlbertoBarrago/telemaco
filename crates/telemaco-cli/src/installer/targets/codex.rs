use std::fs;
use std::path::{Path, PathBuf};

use super::common::*;

/// Codex's home directory: `~/.codex` "unless you set `CODEX_HOME`"
/// (learn.chatgpt.com/docs/agent-configuration/agents-md). With the variable
/// set, everything we wrote under `~/.codex` was read by nothing.
fn codex_home(home: &PathBuf) -> PathBuf {
    home_env_var("CODEX_HOME").unwrap_or_else(|| home.join(".codex"))
}

/// True when this config layer already declares hooks inline.
///
/// Codex merges an inline `[hooks]` table with a `hooks.json` in the same
/// layer and warns at startup, asking for one representation per layer
/// (developers.openai.com/codex/hooks). We keep writing the JSON file, which
/// is the form the other targets share, and say where the warning comes from.
fn has_inline_hooks(content: &str) -> bool {
    content.lines().any(|l| {
        let t = l.trim();
        t == "[hooks]" || t.starts_with("[hooks.") || t.starts_with("[[hooks.")
    })
}

/// The instruction file Codex reads in a directory.
///
/// At every level Codex checks `AGENTS.override.md` first and falls back to
/// `AGENTS.md` only when the override is absent: it "uses only the first
/// non-empty file at this level", and a project directory contributes at most
/// one file (learn.chatgpt.com/docs/agent-configuration/agents-md). A user
/// with an override file therefore never saw the block written to `AGENTS.md`.
fn instructions_file(dir: &Path) -> PathBuf {
    agents_file_in(dir)
}

/// Both instruction files for a directory, for cleanup.
fn all_instruction_files(dir: &Path) -> Vec<PathBuf> {
    all_agents_files_in(dir)
}

pub fn detect(loc: &Location, home: Option<&PathBuf>) -> DetectionResult {
    let (config_dir, toml_path, agents_md) = match loc {
        Location::Global => {
            let h = home.cloned().unwrap_or_else(|| PathBuf::from("."));
            let dir = codex_home(&h);
            (dir.clone(), dir.join("config.toml"), dir.join("AGENTS.md"))
        }
        Location::Folder(folder) => {
            (folder.join(".codex"), folder.join(".codex").join("config.toml"), folder.join("AGENTS.md"))
        }
    };
    // Not `|| agents_md.exists()`: AGENTS.md is the cross-tool convention, so
    // most repositories have one and every one of them came back as a Codex
    // install. Same reasoning as the `.pi` check in the Pi target.
    //
    // A `codex.toml` at the project root is not checked either: the config
    // layers are `.codex/config.toml`, `~/.codex/config.toml` and
    // `/etc/codex/config.toml`, and nothing reads a file by that name at a
    // repository root (developers.openai.com/codex/config-basic).
    let installed = config_dir.exists() || toml_path.exists();
    let mut already_configured = false;
    if toml_path.exists() {
        if let Ok(content) = fs::read_to_string(&toml_path) {
            already_configured = content.contains("[mcp_servers.telemaco]");
        }
    }
    let hint = if already_configured {
        "already configured".to_string()
    } else if installed {
        if loc.is_global() {
            "detected".to_string()
        } else {
            let mut markers = Vec::new();
            if config_dir.exists() { markers.push(".codex/"); }
            if agents_md.exists() { markers.push("AGENTS.md"); }
            if markers.is_empty() { "detected".to_string() } else { markers.join(", ") }
        }
    } else {
        String::new()
    };
    DetectionResult {
        installed,
        already_configured,
        config_path: Some(toml_path),
        hint,
    }
}

pub fn install(loc: &Location, opts: &TargetInstallOptions, home: &PathBuf) -> TargetResult {
    let mut out = Outcome::new(opts.dry_run);

    // 1. Config in ~/.codex/config.toml or <folder>/.codex/config.toml
    let toml_path = match loc {
        Location::Global => codex_home(home).join("config.toml"),
        Location::Folder(folder) => folder.join(".codex").join("config.toml"),
    };
    let mcp_args = stdio_mcp_args(opts.stealth);
    let toml_args_str = mcp_args
        .iter()
        .map(|a| format!("\"{}\"", a))
        .collect::<Vec<_>>()
        .join(", ");
    let toml_existed = toml_path.exists();
    if let Some(original) = out.load_text(&toml_path) {
        let content = original.clone();
        // A bare top-level `hooks` key is where inline hook *tables* live, so
        // the `hooks = true` earlier versions wrote there handed Codex a
        // boolean where it expects a table
        // (developers.openai.com/codex/config-reference). Take it back.
        let (content, legacy_removed) = remove_top_level_key_with_value(&content, "hooks", "true");
        // `[features] hooks` is not ours to set. Hooks are on by default and
        // that key is how a user turns them off
        // (developers.openai.com/codex/hooks), so writing `true` pinned a
        // default at best and overrode a deliberate `false` at worst. Say what
        // is happening instead.
        if toml_table_key_value(&content, "features", "hooks").as_deref() == Some("false") {
            out.note(format!(
                "{} sets [features] hooks = false, so Codex loads no hooks and the Telemaco \
                 prompt hook stays idle until you remove that line.",
                toml_path.display()
            ));
        }
        if has_inline_hooks(&content) {
            out.note(format!(
                "{} already declares hooks inline. Codex merges those with the hooks.json \
                 next to it and warns at startup; move one of the two if you want the \
                 warning gone.",
                toml_path.display()
            ));
        }
        let flag_changed = legacy_removed;
        let managed = [
            ("command", format!("\"{}\"", opts.binary_path)),
            ("args", format!("[{}]", toml_args_str)),
        ];
        let (new_content, action) = upsert_toml_table_keys(&content, "mcp_servers.telemaco", &managed);
        if action != Action::Unchanged || flag_changed {
            let effective_action = match (action, toml_existed) {
                (Action::Unchanged, _) => Action::Updated,
                (Action::Created, true) => Action::Updated,
                (a, _) => a,
            };
            let new_content = with_line_ending(&original, &new_content);
            out.write_text(&toml_path, &new_content, effective_action);
        } else {
            out.push(FileResult { path: toml_path, action: Action::Unchanged });
        }
    }

    // 2. Hooks in ~/.codex/hooks.json or <folder>/.codex/hooks.json
    let hooks_path = match loc {
        Location::Global => codex_home(home).join("hooks.json"),
        Location::Folder(folder) => folder.join(".codex").join("hooks.json"),
    };
    let hooks_existed = hooks_path.exists();
    if let Some(mut hooks_json) = out.load_json(&hooks_path) {
        if add_user_prompt_hook(&mut hooks_json, &prompt_hook_command(&opts.binary_path)) {
            let action = if hooks_existed { Action::Updated } else { Action::Created };
            out.write_json(&hooks_path, &hooks_json, action);
        }
    }

    // 3. Instructions in ~/.codex/AGENTS.md or <folder>/AGENTS.md
    let instructions_path = match loc {
        Location::Global => instructions_file(&codex_home(home)),
        Location::Folder(folder) => instructions_file(folder),
    };
    update_instructions(&mut out, &instructions_path, opts.stealth);

    // Two review steps stand between this install and a hook that runs: a
    // project `.codex/` layer loads only in a project you trust
    // (developers.openai.com/codex/config-basic), and every non-managed hook
    // is trusted by hash, so it is reviewed again whenever its command changes
    // (developers.openai.com/codex/hooks).
    if loc.folder().is_some() {
        out.note(
            "Codex reads a project's .codex/ layer only once you trust the project, so the \
             MCP entry and the prompt hook start working after you answer that prompt.",
        );
    }
    out.note(
        "Codex asks you to review the prompt hook before it runs, and again whenever its \
         command changes, because it records trust against the hook's exact definition.",
    );

    out.finish(TargetId::Codex)
}

pub fn uninstall(loc: &Location, home: &PathBuf, dry_run: bool) -> TargetResult {
    let mut out = Outcome::new(dry_run);

    // Hooks first: whether anything still needs `hooks = true` depends on
    // whether the hooks file survives.
    let hooks_path = match loc {
        Location::Global => codex_home(home).join("hooks.json"),
        Location::Folder(folder) => folder.join(".codex").join("hooks.json"),
    };
    // Whether a hook file is left decides whether `features.hooks` still has a
    // job. Under --dry-run nothing is written, so asking the filesystem again
    // answered for the run that did not happen and the plan came out wrong.
    let mut hooks_remain = hooks_path.exists();
    if hooks_path.exists() {
        if let Some(mut hooks_json) = out.load_json(&hooks_path) {
            if remove_user_prompt_hook(&mut hooks_json) {
                let mut pruned = hooks_json.clone();
                prune_installer_scaffolding(&mut pruned);
                hooks_remain = !pruned.as_object().map_or(false, |o| o.is_empty());
                out.write_json_or_remove(&hooks_path, &hooks_json, Action::Updated);
            }
        }
    }

    let toml_path = match loc {
        Location::Global => codex_home(home).join("config.toml"),
        Location::Folder(folder) => folder.join(".codex").join("config.toml"),
    };
    if toml_path.exists() {
        if let Some(original) = out.load_text(&toml_path) {
            let (content, table_action) = remove_toml_table(&original, "mcp_servers.telemaco");
            // `hooks = true` was ours; take it back only once no hook file is
            // left that would need it.
            let (content, flag_removed) = if hooks_remain {
                (content, false)
            } else {
                let (c, legacy) = remove_top_level_key_with_value(&content, "hooks", "true");
                // Only the `true` an older Telemaco wrote comes out. A `false`
                // is the user's own switch and stays.
                let ours = toml_table_key_value(&c, "features", "hooks").as_deref() == Some("true");
                let (c, feature) = if ours {
                    remove_toml_table_key(&c, "features", "hooks")
                } else {
                    (c, false)
                };
                (c, legacy || feature)
            };
            if table_action == Action::Removed || flag_removed {
                if content.trim().is_empty() {
                    out.remove_config_file(&toml_path, "");
                } else {
                    let rebuilt = with_line_ending(&original, &content);
                    out.write_text(&toml_path, &rebuilt, Action::Removed);
                }
            }
        }
    }

    let dir = match loc {
        Location::Global => codex_home(home),
        Location::Folder(folder) => folder.clone(),
    };
    for path in all_instruction_files(&dir) {
        if path.exists() {
            remove_instructions(&mut out, &path);
        }
    }

    out.finish(TargetId::Codex)
}
