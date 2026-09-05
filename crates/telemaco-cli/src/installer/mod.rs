pub mod instructions;
pub mod json_utils;
pub mod prompt_hook;
pub mod targets;
pub mod text_utils;
pub mod toml_utils;
pub mod yaml_utils;

use std::fs;
use std::io::IsTerminal;
use std::path::PathBuf;
use anyhow::Result;
use inquire::{Confirm, MultiSelect, Select};

use crate::installer::instructions::Action;
use crate::installer::targets::{
    detect_target_in, get_home_dir, install_target_in, resolve_telemaco_binary, tildify,
    uninstall_target_in, Location, LocationArg, TargetId, TargetInstallOptions,
};

pub struct InstallCliArgs {
    pub target: Option<String>,
    pub location: Option<LocationArg>,
    pub folder: Option<PathBuf>,
    pub yes: bool,
    pub stealth: bool,
    pub no_permissions: bool,
    pub print_config: Option<String>,
    pub no_block_web: bool,
    pub dry_run: bool,
}

/// The home directory to resolve every target's global paths against: an
/// explicit override (a `--folder` the user chose to treat as global home)
/// if one was given, otherwise the real `$HOME`. Folder-scoped installs do
/// not use it, so a missing `$HOME` only fails a global install.
fn resolve_install_home(location: &Location, home_override: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(home) = home_override {
        return Ok(home);
    }
    if location.is_global() {
        get_home_dir().ok_or_else(|| {
            anyhow::anyhow!(
                "Cannot resolve your home directory (HOME is not set), so there is no global \
                 location to install into. Set HOME, or pass --folder <dir> and choose Global \
                 home when asked, or Project to configure that directory instead."
            )
        })
    } else {
        Ok(get_home_dir().unwrap_or_else(|| PathBuf::from(".")))
    }
}

pub fn run_installer(args: InstallCliArgs) -> Result<()> {
    if let Some(target_str) = args.print_config.as_deref() {
        return print_config_snippet(target_str, args.stealth, !args.no_permissions);
    }

    println!();
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ 🌐 Telemaco AI Assistant Installer                          │");
    println!("│ Configures AI models & coding agents to browse via Telemaco │");
    println!("└─────────────────────────────────────────────────────────────┘");
    println!();

    let is_interactive = !args.yes && std::io::stdin().is_terminal() && std::io::stdout().is_terminal();

    let mut explicit_home: Option<PathBuf> = None;

    // Determine target location (Folder or Global)
    let location = if let Some(folder_path) = args.folder {
        let resolved = if folder_path.is_absolute() {
            folder_path
        } else {
            std::env::current_dir()?.join(folder_path)
        };
        if !resolved.exists() {
            anyhow::bail!("Target directory '{}' does not exist", resolved.display());
        }
        if !resolved.is_dir() {
            anyhow::bail!("Target path '{}' is not a directory", resolved.display());
        }
        let resolved = resolved.canonicalize().unwrap_or(resolved);

        // A folder is ambiguous: a project to configure, or the directory a
        // global install should treat as home instead of $HOME (a relocated
        // Claude Code config dir, say). --location settles it without
        // asking; otherwise ask, since guessing wrong writes into the wrong
        // scope entirely.
        let as_global = match args.location {
            Some(LocationArg::Global) => true,
            Some(LocationArg::Local) => false,
            None if is_interactive => {
                let choice = Select::new(
                    &format!("Configure {} as:", tildify(&resolved)),
                    vec![
                        "Project folder (writes config inside it)",
                        "Global home (used instead of $HOME for a global install)",
                    ],
                )
                .prompt();
                match choice {
                    Ok(c) => c.starts_with("Global"),
                    Err(_) => {
                        println!("Installation cancelled.");
                        return Ok(());
                    }
                }
            }
            None => false,
        };

        if as_global {
            explicit_home = Some(resolved);
            Location::Global
        } else {
            Location::Folder(resolved)
        }
    } else if let Some(loc_arg) = args.location {
        match loc_arg {
            LocationArg::Global => Location::Global,
            LocationArg::Local => {
                let cur = std::env::current_dir()?.canonicalize().unwrap_or_else(|_| std::env::current_dir().unwrap());
                Location::Folder(cur)
            }
        }
    } else if !is_interactive {
        Location::Global
    } else {
        let loc_choice = Select::new(
            "Where would you like to install Telemaco?",
            vec![
                "All projects (Global: ~/.claude.json, ~/.cursor, etc.)",
                "Current project folder (Local: .mcp.json, .cursor, AGENTS.md, etc.)",
            ],
        )
        .prompt();

        match loc_choice {
            Ok(choice) if choice.starts_with("All") => Location::Global,
            Ok(_) => {
                let cur = std::env::current_dir()?.canonicalize().unwrap_or_else(|_| std::env::current_dir().unwrap());
                Location::Folder(cur)
            }
            Err(_) => {
                println!("Installation cancelled.");
                return Ok(());
            }
        }
    };

    let home = resolve_install_home(&location, explicit_home)?;
    if location.is_global() && home != get_home_dir().unwrap_or_default() {
        println!("🏠 Global home: {}", home.display());
    }

    // 1. Detection
    if let Location::Folder(ref folder) = location {
        println!("📁 Target folder: {}", folder.display());
        println!("🔍 Scanning folder for AI assistant configurations...");
    } else {
        println!("🔍 Scanning for installed AI coding agents...");
    }

    let mut detected_list = Vec::new();
    for &id in TargetId::all() {
        let detection = detect_target_in(id, &location, Some(&home));
        if detection.installed {
            let path_display = detection
                .config_path
                .as_ref()
                .map(|p| tildify(p))
                .unwrap_or_default();
            let status = if detection.already_configured {
                " [already configured]"
            } else {
                ""
            };
            let marker_info = if !detection.hint.is_empty() && detection.hint != "already configured" && detection.hint != "detected" {
                format!(" (found {})", detection.hint)
            } else if !path_display.is_empty() {
                format!(" ({})", path_display)
            } else {
                String::new()
            };
            println!("  ✔ Found {}{}{}", id.display_name(), status, marker_info);
            detected_list.push(id);
        }
    }
    if detected_list.is_empty() {
        if location.is_global() {
            println!("  ℹ No existing agent configurations detected at standard paths.");
        } else {
            println!("  ℹ No agent configuration files or markers found in this folder.");
        }
    }
    println!();

    // 2. Target selection
    let selected_targets: Vec<TargetId> = if let Some(target_str) = args.target.as_deref() {
        resolve_target_arg(target_str, &detected_list)?
    } else if !is_interactive {
        if detected_list.is_empty() {
            vec![TargetId::Claude]
        } else {
            detected_list.clone()
        }
    } else {
        // Virtual GUI / Interactive MultiSelect
        struct TargetOption {
            id: TargetId,
            label: String,
        }

        impl std::fmt::Display for TargetOption {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.label)
            }
        }

        let options: Vec<TargetOption> = TargetId::all()
            .iter()
            .map(|&id| {
                let detection = detect_target_in(id, &location, Some(&home));
                let tag = if !detection.hint.is_empty() {
                    format!(" ({})", detection.hint)
                } else {
                    String::new()
                };
                TargetOption {
                    id,
                    label: format!("{}{}", id.display_name(), tag),
                }
            })
            .collect();

        let default_indices: Vec<usize> = options
            .iter()
            .enumerate()
            .filter(|(_, opt)| detected_list.contains(&opt.id))
            .map(|(idx, _)| idx)
            .collect();

        let initial_defaults = if default_indices.is_empty() {
            vec![0] // Claude Code default
        } else {
            default_indices
        };

        let prompt_msg = if location.is_global() {
            "Which AI coding assistants would you like to configure?"
        } else {
            "Which AI coding assistants would you like to configure for this folder?"
        };

        let ans = MultiSelect::new(prompt_msg, options)
            .with_default(&initial_defaults)
            .with_help_message("Space to select, Enter to confirm, Arrow keys to navigate")
            .prompt();

        match ans {
            Ok(selected) => {
                if selected.is_empty() {
                    println!("No agents selected. Exiting.");
                    return Ok(());
                }
                selected.into_iter().map(|opt| opt.id).collect()
            }
            Err(_) => {
                println!("Installation cancelled.");
                return Ok(());
            }
        }
    };

    if selected_targets.is_empty() {
        println!("No agents selected. Nothing to do.");
        return Ok(());
    }

    println!(
        "Selected {} agent(s): {}",
        selected_targets.len(),
        selected_targets
            .iter()
            .map(|t| t.display_name())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!();

    // 3. Binary resolution and optional ~/.local/bin symlink
    let mut binary_path = resolve_telemaco_binary();
    if is_interactive && location == Location::Global {
        {
            let local_bin = home.join(".local").join("bin").join("telemaco");
            if let Ok(cur_exe) = std::env::current_exe() {
                // Path inequality is not enough: reached through another
                // symlink (or a symlinked HOME), `cur_exe` and `local_bin` can
                // be two names for the same file, and removing one deletes the
                // binary we are about to link to.
                let same_file = fs::canonicalize(&cur_exe)
                    .ok()
                    .zip(fs::canonicalize(&local_bin).ok())
                    .map_or(false, |(a, b)| a == b);
                if cur_exe != local_bin && !same_file {
                    let confirm = Confirm::new(
                        &format!(
                            "Install/symlink telemaco CLI binary to {}?",
                            tildify(&local_bin)
                        ),
                    )
                    .with_default(true)
                    .with_help_message("Required so GUI apps (Cursor, Windsurf, Antigravity) find Telemaco reliably")
                    .prompt();

                    if let Ok(true) = confirm {
                        if let Some(parent) = local_bin.parent() {
                            let _ = fs::create_dir_all(parent);
                        }
                        let _ = fs::remove_file(&local_bin);
                        #[cfg(unix)]
                        let linked = std::os::unix::fs::symlink(&cur_exe, &local_bin).is_ok();
                        #[cfg(not(unix))]
                        let linked = false;
                        if linked {
                            println!("  ✔ Symlinked telemaco -> {}", tildify(&local_bin));
                            binary_path = local_bin.display().to_string();
                        } else if fs::copy(&cur_exe, &local_bin).is_ok() {
                            println!("  ✔ Copied telemaco to {}", tildify(&local_bin));
                            binary_path = local_bin.display().to_string();
                        }
                    }
                }
            }
        }
    }

    // 4. Stealth mode option
    let stealth = if args.stealth {
        true
    } else if !is_interactive {
        true // default to stealth
    } else {
        let stealth_confirm = Confirm::new(
            "Enable stealth mode by default for MCP? (Anti-bot bypass & consistent browser fingerprint)",
        )
        .with_default(true)
        .prompt();

        match stealth_confirm {
            Ok(val) => val,
            Err(_) => {
                println!("Installation cancelled.");
                return Ok(());
            }
        }
    };

    // 5. Auto-allow permissions (Claude Code, Kiro)
    let auto_allow = if selected_targets.contains(&TargetId::Claude) || selected_targets.contains(&TargetId::Kiro) {
        if args.no_permissions {
            false
        } else if !is_interactive {
            true
        } else {
            let perm_confirm = Confirm::new(
                "Auto-approve Telemaco commands in Claude Code / Kiro? (Skips repetitive confirmation prompts)",
            )
            .with_default(true)
            .prompt();

            match perm_confirm {
                Ok(val) => val,
                Err(_) => {
                    println!("Installation cancelled.");
                    return Ok(());
                }
            }
        }
    } else {
        false
    };

    // 6. Refusing the agent's built-in web tools is the point of the tool, but
    //    it disables something the user already had, so it is stated and can be
    //    declined.
    let blocks_web = selected_targets.contains(&TargetId::Claude)
        || selected_targets.contains(&TargetId::Poolside);
    let block_builtin_web = if args.no_block_web {
        false
    } else if !is_interactive || !blocks_web {
        true
    } else {
        let confirm = Confirm::new(
            "Block the agent's built-in web search/fetch so it must go through Telemaco?",
        )
        .with_default(true)
        .with_help_message("Adds a PreToolUse guard; undo any time with --no-block-web or `telemaco uninstall`")
        .prompt();
        match confirm {
            Ok(val) => val,
            Err(_) => {
                println!("Installation cancelled.");
                return Ok(());
            }
        }
    };

    println!();
    if args.dry_run {
        println!("🔍 Dry run: nothing will be written.");
    }
    if let Location::Folder(ref f) = location {
        println!("🚀 Applying Telemaco configuration to {}...", f.display());
    } else {
        println!("🚀 Applying Telemaco configuration...");
    }
    println!();

    let install_opts = TargetInstallOptions {
        auto_allow,
        stealth,
        binary_path,
        block_builtin_web,
        dry_run: args.dry_run,
    };

    for target in selected_targets {
        let res = install_target_in(target, &location, &install_opts, &home);
        for file in res.files {
            let verb = match (args.dry_run, file.action) {
                (_, Action::Unchanged) => "Unchanged",
                (_, Action::NotFound) => "Not found",
                (true, Action::Created) => "Would create",
                (true, Action::Updated) => "Would update",
                (true, Action::Removed) => "Would remove",
                (false, Action::Created) => "Created",
                (false, Action::Updated) => "Updated",
                (false, Action::Removed) => "Removed",
            };
            println!("  ✔ {}: {} {}", res.display_name, verb, tildify(&file.path));
        }
        for note in res.notes {
            println!("  ℹ {}: {}", res.display_name, note);
        }
    }

    println!();
    if args.dry_run {
        println!("🔍 Dry run complete. Re-run without --dry-run to apply.");
        return Ok(());
    }
    println!("✨ Installation complete!");
    println!("👉 Restart your AI coding agent(s) so they load the new Telemaco tools and directives.");
    println!("💡 Models will now strictly use Telemaco instead of generic web search!");
    println!();

    Ok(())
}

pub fn run_uninstaller(args: InstallCliArgs) -> Result<()> {
    println!();
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ 🗑️  Telemaco Uninstaller                                     │");
    println!("│ Removes Telemaco from AI assistant configs and instructions │");
    println!("└─────────────────────────────────────────────────────────────┘");
    println!();

    let is_interactive = !args.yes && std::io::stdin().is_terminal() && std::io::stdout().is_terminal();

    let mut explicit_home: Option<PathBuf> = None;

    let location = if let Some(folder_path) = args.folder {
        let resolved = if folder_path.is_absolute() {
            folder_path
        } else {
            std::env::current_dir()?.join(folder_path)
        };
        let resolved = resolved.canonicalize().unwrap_or(resolved);

        let as_global = match args.location {
            Some(LocationArg::Global) => true,
            Some(LocationArg::Local) => false,
            None if is_interactive => {
                let choice = Select::new(
                    &format!("Configure {} as:", tildify(&resolved)),
                    vec![
                        "Project folder (config written inside it)",
                        "Global home (used instead of $HOME for a global install)",
                    ],
                )
                .prompt();
                match choice {
                    Ok(c) => c.starts_with("Global"),
                    Err(_) => {
                        println!("Uninstallation cancelled.");
                        return Ok(());
                    }
                }
            }
            None => false,
        };

        if as_global {
            explicit_home = Some(resolved);
            Location::Global
        } else {
            Location::Folder(resolved)
        }
    } else if let Some(loc_arg) = args.location {
        match loc_arg {
            LocationArg::Global => Location::Global,
            LocationArg::Local => {
                let cur = std::env::current_dir()?.canonicalize().unwrap_or_else(|_| std::env::current_dir().unwrap());
                Location::Folder(cur)
            }
        }
    } else {
        Location::Global
    };

    let home = resolve_install_home(&location, explicit_home)?;
    if location.is_global() && home != get_home_dir().unwrap_or_default() {
        println!("🏠 Global home: {}", home.display());
    }

    let targets_to_uninstall: Vec<TargetId> = if let Some(target_str) = args.target.as_deref() {
        resolve_target_arg(target_str, &[])?
    } else if !is_interactive {
        TargetId::all().to_vec()
    } else {
        let confirm = Confirm::new("Remove Telemaco MCP configuration from all detected agents?")
            .with_default(true)
            .prompt();
        if let Ok(true) = confirm {
            TargetId::all().to_vec()
        } else {
            println!("Uninstallation cancelled.");
            return Ok(());
        }
    };

    if args.dry_run {
        println!("🔍 Dry run: nothing will be written.");
    }
    let mut backups: Vec<PathBuf> = Vec::new();
    for target in targets_to_uninstall {
        let res = uninstall_target_in(target, &location, &home, args.dry_run);
        for file in &res.files {
            let backup = crate::installer::json_utils::backup_path(&file.path);
            if backup.exists() && !backups.contains(&backup) {
                backups.push(backup);
            }
        }
        for file in res.files {
            let verb = if args.dry_run { "Would remove from" } else { "Removed from" };
            println!("  ✔ {}: {} {}", res.display_name, verb, tildify(&file.path));
        }
        for note in res.notes {
            println!("  ℹ {}: {}", res.display_name, note);
        }
    }

    if !backups.is_empty() {
        println!();
        println!("  ℹ {} backup file(s) kept, delete them when you are happy:", backups.len());
        for b in &backups {
            println!("      {}", tildify(b));
        }
    }

    println!();
    println!("Telemaco MCP server removed. Restart your agents to apply.");
    println!();
    Ok(())
}

fn resolve_target_arg(val: &str, detected: &[TargetId]) -> Result<Vec<TargetId>> {
    let trimmed = val.trim();
    if trimmed.eq_ignore_ascii_case("all") {
        return Ok(TargetId::all().to_vec());
    }
    if trimmed.eq_ignore_ascii_case("auto") {
        return if detected.is_empty() {
            Ok(vec![TargetId::Claude])
        } else {
            Ok(detected.to_vec())
        };
    }
    if trimmed.eq_ignore_ascii_case("none") {
        return Ok(vec![]);
    }

    let mut result = Vec::new();
    for part in trimmed.split(',') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        match TargetId::parse(p) {
            Some(id) => result.push(id),
            None => {
                let known: Vec<&str> = TargetId::all().iter().map(|t| t.id_str()).collect();
                anyhow::bail!(
                    "Unknown target ID '{}'. Known: {}, or 'auto'/'all'/'none'.",
                    p,
                    known.join(", ")
                );
            }
        }
    }
    Ok(result)
}

fn print_config_snippet(target_str: &str, stealth: bool, auto_allow: bool) -> Result<()> {
    let target = TargetId::parse(target_str)
        .ok_or_else(|| anyhow::anyhow!("Unknown target '{}'", target_str))?;
    println!(
        "{}",
        crate::installer::targets::config_snippet(
            target,
            &resolve_telemaco_binary(),
            stealth,
            auto_allow,
        )
    );
    Ok(())
}

#[cfg(test)]
mod install_home_tests {
    use super::*;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(label: &str) -> Self {
            let mut p = std::env::temp_dir();
            p.push(format!(
                "telemaco_home_override_{}_{}",
                label,
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn path(&self) -> &PathBuf {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn test_resolve_install_home_prefers_the_override() {
        let dir = TempDir::new("prefer_override");
        let home = resolve_install_home(&Location::Global, Some(dir.path().clone())).unwrap();
        assert_eq!(&home, dir.path());
    }

    #[test]
    fn test_resolve_install_home_folder_scope_never_needs_home() {
        // A folder install must not fail just because $HOME cannot be
        // resolved: the target logic ignores `home` in that branch.
        let loc = Location::Folder(PathBuf::from("/tmp/some-project"));
        assert!(resolve_install_home(&loc, None).is_ok());
    }
}
