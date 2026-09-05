use std::fs;
use std::path::{Path, PathBuf};

use super::common::*;

/// Hermes' home directory: `~/.hermes` unless `$HERMES_HOME` says otherwise.
///
/// The docs use `$HERMES_HOME` throughout ("`~/.hermes/SOUL.md` or
/// `$HERMES_HOME/SOUL.md` if you run Hermes with a custom home directory"), and
/// the config, the secrets file and the MCP tokens all live under it
/// (hermes-agent.nousresearch.com/docs/user-guide/features/context-files,
/// /docs/reference/mcp-config-reference).
fn hermes_home(home: &PathBuf) -> PathBuf {
    home_env_var("HERMES_HOME").unwrap_or_else(|| home.join(".hermes"))
}

/// Both the MCP servers and the shell hooks live in one YAML file.
fn config_path(home: &PathBuf) -> PathBuf {
    hermes_home(home).join("config.yaml")
}

/// The project context file Hermes actually reads.
///
/// Hermes scans the working directory for `.hermes.md`, then `AGENTS.md`, then
/// `CLAUDE.md`, then `.cursorrules`, and the **first match wins** - only one of
/// them is loaded (hermes-agent.nousresearch.com/docs/user-guide/features/context-files).
/// So the block has to go in whichever file Hermes would pick, or it is written
/// somewhere that is never read. With none of them present, `AGENTS.md` is the
/// one to create: it is the file the other agents share.
fn project_context_file(folder: &Path) -> PathBuf {
    for name in [".hermes.md", "AGENTS.md", "CLAUDE.md", ".cursorrules"] {
        let candidate = folder.join(name);
        if candidate.exists() {
            return candidate;
        }
    }
    folder.join("AGENTS.md")
}

/// Every project file we may have written into, for cleanup.
fn all_project_context_files(folder: &Path) -> Vec<PathBuf> {
    [".hermes.md", "AGENTS.md", "CLAUDE.md", ".cursorrules"]
        .iter()
        .map(|n| folder.join(n))
        .collect()
}

pub fn detect(loc: &Location, home: Option<&PathBuf>) -> DetectionResult {
    let h = home.cloned().unwrap_or_else(|| PathBuf::from("."));
    let cfg = config_path(&h);
    let installed = match loc {
        Location::Global => hermes_home(&h).exists() || cfg.exists(),
        // `.hermes.md` is the only project file Hermes owns; the other three it
        // reads belong to the cross-tool conventions or to Cursor.
        Location::Folder(folder) => folder.join(".hermes.md").exists(),
    };

    let mut already_configured = false;
    if cfg.exists() {
        if let Ok(content) = fs::read_to_string(&cfg) {
            already_configured = content.lines().any(|l| l.trim() == "telemaco:");
        }
    }
    if !already_configured {
        if let Location::Folder(folder) = loc {
            let ctx = project_context_file(folder);
            if ctx.exists() {
                already_configured = fs::read_to_string(&ctx)
                    .map_or(false, |c| c.contains("<!-- TELEMACO_START -->"));
            }
        }
    }

    let hint = if already_configured {
        "already configured".to_string()
    } else if installed {
        if loc.is_global() {
            "detected".to_string()
        } else {
            ".hermes.md".to_string()
        }
    } else {
        String::new()
    };

    DetectionResult {
        installed,
        already_configured,
        config_path: Some(cfg),
        hint,
    }
}

/// The `pre_llm_call` entry, written at indent zero.
///
/// `pre_llm_call` is the event that can "inject context into the next LLM
/// turn"; a hook answers with `{"context": "..."}` on stdout, which is what
/// `--format hermes` prints. `timeout` is in seconds, default 60, capped at 300
/// (hermes-agent.nousresearch.com/docs/user-guide/features/hooks).
fn hook_item(binary_path: &str) -> String {
    format!(
        "- command: \"{} prompt-hook --format hermes\"\n  timeout: 30",
        binary_path
    )
}

pub fn install(loc: &Location, opts: &TargetInstallOptions, home: &PathBuf) -> TargetResult {
    let mut out = Outcome::new(opts.dry_run);

    // Hermes has no project-level config: the servers and the hooks are read
    // from the one file under its home, so a folder install configures the
    // context file only.
    if loc.is_global() {
        let cfg = config_path(home);
        let mcp_entry = yaml_mcp_entry("telemaco", &opts.binary_path, &stdio_mcp_args(opts.stealth));
        let hook = hook_item(&opts.binary_path);

        if !cfg.exists() {
            let content = format!(
                "mcp_servers:\n{}\n\nhooks:\n  pre_llm_call:\n{}\n",
                indent_block(&mcp_entry, 2),
                indent_block(&hook, 4)
            );
            out.write_text(&cfg, &content, Action::Created);
        } else if let Some(original) = out.load_text(&cfg) {
            let mut content = original.clone();
            let mut modified = false;

            let has_entry = content.lines().any(|l| l.trim() == "telemaco:");
            if has_entry {
                if let Some(updated) = refresh_yaml_entry(&content, "telemaco", &mcp_entry) {
                    if updated != content {
                        content = updated;
                        modified = true;
                    }
                }
            } else {
                match upsert_yaml_path(&content, &["mcp_servers"], &mcp_entry) {
                    Ok(updated) => {
                        content = updated;
                        modified = true;
                    }
                    Err(e) => out.note(format!("{}: {}", cfg.display(), e)),
                }
            }

            // Our hook is replaced rather than skipped, so a moved binary is
            // followed the way every other target follows it.
            let (without, removed) = remove_yaml_block(&content, |l| {
                l.trim().starts_with("- command:") && is_telemaco_hook_command(l)
            });
            if removed {
                content = without;
            }
            match upsert_yaml_path(&content, &["hooks", "pre_llm_call"], &hook) {
                Ok(updated) => {
                    if updated != content {
                        content = updated;
                        modified = true;
                    }
                }
                Err(e) => out.note(format!("{}: {}", cfg.display(), e)),
            }

            if modified {
                let rebuilt = with_line_ending(&original, &content);
                out.write_text(&cfg, &rebuilt, Action::Updated);
            } else {
                out.push(FileResult { path: cfg, action: Action::Unchanged });
            }
        }

        // `SOUL.md` is the only global context file, and it holds the agent's
        // personality: the directive does not belong there. The hook covers
        // every session instead.
    }

    let instructions_path = match loc {
        Location::Global => None,
        Location::Folder(folder) => Some(project_context_file(folder)),
    };
    if let Some(path) = instructions_path {
        update_instructions(&mut out, &path, opts.stealth);
    }

    out.finish(TargetId::Hermes)
}

pub fn uninstall(loc: &Location, home: &PathBuf, dry_run: bool) -> TargetResult {
    let mut out = Outcome::new(dry_run);

    if loc.is_global() {
        let cfg = config_path(home);
        if cfg.exists() {
            if let Some(original) = out.load_text(&cfg) {
                let (content, removed_mcp) =
                    remove_yaml_block(&original, |l| l.trim() == "telemaco:");
                let (content, removed_hook) = remove_yaml_block(&content, |l| {
                    l.trim().starts_with("- command:") && is_telemaco_hook_command(l)
                });
                if removed_mcp || removed_hook {
                    let pruned =
                        prune_empty_yaml_keys(&content, &["mcp_servers", "hooks", "pre_llm_call"]);
                    if pruned.trim().is_empty() {
                        out.remove_config_file(&cfg, "");
                    } else {
                        let rebuilt = with_line_ending(&original, &format!("{}\n", pruned.trim_end()));
                        out.write_text(&cfg, &rebuilt, Action::Removed);
                    }
                }
            }
        }
    }

    if let Location::Folder(folder) = loc {
        for path in all_project_context_files(folder) {
            if path.exists() {
                remove_instructions(&mut out, &path);
            }
        }
    }

    out.finish(TargetId::Hermes)
}
