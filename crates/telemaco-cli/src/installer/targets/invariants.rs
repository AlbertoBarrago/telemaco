//! Rules every target has to obey, checked against every target.
//!
//! The per-target tests next door each grew out of one bug in one agent. That
//! is how a fix for Qwen missed Gemini and a fix for Windsurf's writer missed
//! its detector: the rule was real, the test only ever asked one target. Each
//! probe here takes a rule that was learned the expensive way and runs it over
//! `TargetId::all()`, in both locations, so the next target to break it is
//! named by the failure.

use super::*;
use crate::installer::instructions::Action;
use std::collections::BTreeMap;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Sandbox {
    root: PathBuf,
    home: PathBuf,
    project: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "telemaco_inv_{}_{}_{}",
            name,
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&root);
        let home = root.join("home");
        let project = root.join("project");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&project).unwrap();
        Self { root, home, project }
    }

    /// The two places an install can land, each with a home of its own so a
    /// folder install that leaks into $HOME shows up as a stray file.
    fn location(&self, global: bool) -> Location {
        if global {
            Location::Global
        } else {
            Location::Folder(self.project.clone())
        }
    }

    /// Where the target is allowed to write for this location.
    fn scope(&self, global: bool) -> &Path {
        if global { &self.home } else { &self.project }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn opts(binary: &str, dry_run: bool) -> TargetInstallOptions {
    TargetInstallOptions {
        auto_allow: true,
        stealth: true,
        binary_path: binary.to_string(),
        block_builtin_web: true,
        dry_run,
    }
}

/// Every regular file under `dir`, symlinks included, backups excluded: a
/// backup is a deliberate leftover, everything else has to be accounted for.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let meta = match fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.is_dir() {
                walk(&path, out);
            } else if !path.to_string_lossy().ends_with(".telemaco-backup") {
                out.push(path);
            }
        }
    }
    walk(dir, &mut out);
    out.sort();
    out
}

fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    files_under(dir)
        .into_iter()
        .map(|p| {
            let bytes = fs::read(&p).unwrap_or_default();
            (p, bytes)
        })
        .collect()
}

/// Both locations for one target, as `(label, global)`.
const LOCATIONS: &[(&str, bool)] = &[("folder", false), ("global", true)];

#[test]
fn every_target_round_trips_to_nothing() {
    for &target in TargetId::all() {
        for &(label, global) in LOCATIONS {
            let sb = Sandbox::new("roundtrip");
            let loc = sb.location(global);
            install_target_in(target, &loc, &opts("telemaco", false), &sb.home);
            uninstall_target_in(target, &loc, &sb.home, false);

            let left = files_under(&sb.root);
            assert!(
                left.is_empty(),
                "{} ({}) left {:?} behind",
                target.id_str(),
                label,
                left
            );
        }
    }
}

#[test]
fn every_target_stays_inside_its_location() {
    // A folder install belongs to the folder. A global install belongs to the
    // home. Anything written on the other side of that line is a target that
    // ignored the location it was handed.
    for &target in TargetId::all() {
        for &(label, global) in LOCATIONS {
            let sb = Sandbox::new("scope");
            let loc = sb.location(global);
            let res = install_target_in(target, &loc, &opts("telemaco", false), &sb.home);

            let scope = sb.scope(global).to_path_buf();
            for file in &res.files {
                assert!(
                    file.path.starts_with(&scope),
                    "{} ({}) reported {} outside {}",
                    target.id_str(),
                    label,
                    file.path.display(),
                    scope.display()
                );
            }
            let outside: Vec<_> = files_under(&sb.root)
                .into_iter()
                .filter(|p| !p.starts_with(&scope))
                .collect();
            assert!(
                outside.is_empty(),
                "{} ({}) wrote outside {}: {:?}",
                target.id_str(),
                label,
                scope.display(),
                outside
            );
        }
    }
}

#[test]
fn every_target_reports_what_it_actually_wrote() {
    // A reported Created or Updated that is not on disk is a lie in the
    // summary the user reads, and a written file that is not reported is a
    // file uninstall may never come back for.
    for &target in TargetId::all() {
        for &(label, global) in LOCATIONS {
            let sb = Sandbox::new("reported");
            let loc = sb.location(global);
            let res = install_target_in(target, &loc, &opts("telemaco", false), &sb.home);

            for file in &res.files {
                if matches!(file.action, Action::Created | Action::Updated) {
                    assert!(
                        file.path.exists(),
                        "{} ({}) reported {:?} for {} but did not write it",
                        target.id_str(),
                        label,
                        file.action,
                        file.path.display()
                    );
                }
            }

            let reported: Vec<_> = res.files.iter().map(|f| f.path.clone()).collect();
            for path in files_under(&sb.root) {
                assert!(
                    reported.contains(&path),
                    "{} ({}) wrote {} without reporting it",
                    target.id_str(),
                    label,
                    path.display()
                );
            }
        }
    }
}

#[test]
fn every_target_is_idempotent() {
    for &target in TargetId::all() {
        for &(label, global) in LOCATIONS {
            let sb = Sandbox::new("idempotent");
            let loc = sb.location(global);
            install_target_in(target, &loc, &opts("telemaco", false), &sb.home);
            let first = snapshot(&sb.root);

            let res = install_target_in(target, &loc, &opts("telemaco", false), &sb.home);
            let second = snapshot(&sb.root);

            assert_eq!(
                first.keys().collect::<Vec<_>>(),
                second.keys().collect::<Vec<_>>(),
                "{} ({}) changed the file set on reinstall",
                target.id_str(),
                label
            );
            for (path, bytes) in &first {
                assert_eq!(
                    bytes,
                    second.get(path).unwrap(),
                    "{} ({}) rewrote {} on reinstall:\n--- first ---\n{}\n--- second ---\n{}",
                    target.id_str(),
                    label,
                    path.display(),
                    String::from_utf8_lossy(bytes),
                    String::from_utf8_lossy(second.get(path).unwrap())
                );
            }
            for file in &res.files {
                assert_ne!(
                    file.action,
                    Action::Created,
                    "{} ({}) claims it created {} twice",
                    target.id_str(),
                    label,
                    file.path.display()
                );
            }
        }
    }
}

#[test]
fn every_target_dry_run_matches_the_real_thing() {
    // The dry run is the only thing between the user and a surprise. It has to
    // name the same files the real install touches, and touch none of them.
    for &target in TargetId::all() {
        for &(label, global) in LOCATIONS {
            let dry_sb = Sandbox::new("dryrun");
            let dry_loc = dry_sb.location(global);
            let dry = install_target_in(target, &dry_loc, &opts("telemaco", true), &dry_sb.home);
            let touched = files_under(&dry_sb.root);
            assert!(
                touched.is_empty(),
                "{} ({}) dry run wrote {:?}",
                target.id_str(),
                label,
                touched
            );

            let wet_sb = Sandbox::new("wetrun");
            let wet_loc = wet_sb.location(global);
            let wet = install_target_in(target, &wet_loc, &opts("telemaco", false), &wet_sb.home);

            let plan: Vec<String> = dry
                .files
                .iter()
                .map(|f| {
                    f.path
                        .strip_prefix(&dry_sb.root)
                        .unwrap_or(&f.path)
                        .display()
                        .to_string()
                })
                .collect();
            let done: Vec<String> = wet
                .files
                .iter()
                .map(|f| {
                    f.path
                        .strip_prefix(&wet_sb.root)
                        .unwrap_or(&f.path)
                        .display()
                        .to_string()
                })
                .collect();
            assert_eq!(
                plan,
                done,
                "{} ({}) dry run does not match the install it describes",
                target.id_str(),
                label
            );
        }
    }
}

#[test]
fn every_target_answers_for_what_it_installed() {
    // Detection reads the files install writes. When only one of the two
    // learns a new path, `telemaco install` reports a target as unconfigured
    // right after configuring it.
    for &target in TargetId::all() {
        for &(label, global) in LOCATIONS {
            let sb = Sandbox::new("detect");
            let loc = sb.location(global);

            let before = detect_target_in(target, &loc, Some(&sb.home));
            assert!(
                !before.already_configured,
                "{} ({}) claims to be configured in an empty tree",
                target.id_str(),
                label
            );

            install_target_in(target, &loc, &opts("telemaco", false), &sb.home);
            let after = detect_target_in(target, &loc, Some(&sb.home));
            assert!(
                after.already_configured,
                "{} ({}) does not recognise its own install",
                target.id_str(),
                label
            );

            uninstall_target_in(target, &loc, &sb.home, false);
            let gone = detect_target_in(target, &loc, Some(&sb.home));
            assert!(
                !gone.already_configured,
                "{} ({}) still reports itself configured after uninstall",
                target.id_str(),
                label
            );
        }
    }
}

#[test]
fn no_target_orphans_a_symlinked_config() {
    // Configs that live in a dotfiles repo are reached through a symlink. An
    // install that replaces the link, or an uninstall that removes it, breaks
    // the user's setup in a way they only notice much later.
    for &target in TargetId::all() {
        for &(label, global) in LOCATIONS {
            let sb = Sandbox::new("symlink");
            let loc = sb.location(global);
            install_target_in(target, &loc, &opts("telemaco", false), &sb.home);

            let dotfiles = sb.root.join("dotfiles");
            fs::create_dir_all(&dotfiles).unwrap();
            let mut linked = Vec::new();
            for (i, path) in files_under(&sb.root).into_iter().enumerate() {
                if path.starts_with(&dotfiles) {
                    continue;
                }
                let stashed = dotfiles.join(format!("{}_{}", i, path.file_name().unwrap().to_string_lossy()));
                fs::rename(&path, &stashed).unwrap();
                #[cfg(unix)]
                std::os::unix::fs::symlink(&stashed, &path).unwrap();
                #[cfg(not(unix))]
                fs::copy(&stashed, &path).unwrap();
                linked.push((path, stashed));
            }

            install_target_in(target, &loc, &opts("telemaco", false), &sb.home);
            for (link, stashed) in &linked {
                assert!(
                    stashed.exists(),
                    "{} ({}) install broke the link {} -> {}",
                    target.id_str(),
                    label,
                    link.display(),
                    stashed.display()
                );
            }

            uninstall_target_in(target, &loc, &sb.home, false);
            for (link, stashed) in &linked {
                let meta = fs::symlink_metadata(link);
                if let Ok(meta) = meta {
                    if meta.file_type().is_symlink() {
                        assert!(
                            link.exists(),
                            "{} ({}) left {} pointing at a deleted {}",
                            target.id_str(),
                            label,
                            link.display(),
                            stashed.display()
                        );
                    }
                }
            }
        }
    }
}

/// What a file of this kind looks like before Telemaco ever ran, carrying one
/// thing of the user's that has to still be there afterwards.
fn seed_for(path: &Path) -> Option<&'static str> {
    // A path we named after ourselves is ours: install creates it, uninstall
    // takes it away again. Everything else is a config we are a guest in.
    if path.to_string_lossy().contains("telemaco") {
        return None;
    }
    match path.extension().and_then(|e| e.to_str()) {
        Some("json") => Some("{\n  \"userOwnedKey\": \"keep me\"\n}\n"),
        Some("toml") => Some("userOwnedKey = \"keep me\"\n"),
        Some("yaml") | Some("yml") => Some("userOwnedKey: \"keep me\"\n"),
        Some("md") | None => Some("User prose that predates telemaco.\n"),
        _ => None,
    }
}

/// The paths a target writes for a location, relative to the sandbox root.
fn paths_written(target: TargetId, global: bool) -> Vec<PathBuf> {
    let sb = Sandbox::new("scout");
    let loc = sb.location(global);
    install_target_in(target, &loc, &opts("telemaco", false), &sb.home);
    files_under(&sb.root)
        .into_iter()
        .map(|p| p.strip_prefix(&sb.root).unwrap().to_path_buf())
        .collect()
}

#[test]
fn no_target_evicts_a_file_it_found_there() {
    // Every config a target touches is pre-created with something of the
    // user's in it. Install, reinstall and uninstall all have to leave it: we
    // are a guest in every file we did not name after ourselves.
    for &target in TargetId::all() {
        for &(label, global) in LOCATIONS {
            let seeds: Vec<(PathBuf, &str)> = paths_written(target, global)
                .into_iter()
                .filter_map(|rel| seed_for(&rel).map(|seed| (rel, seed)))
                .collect();
            if seeds.is_empty() {
                continue;
            }

            let sb = Sandbox::new("guest");
            let loc = sb.location(global);
            for (rel, seed) in &seeds {
                let path = sb.root.join(rel);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, seed).unwrap();
            }

            let check = |stage: &str, sb: &Sandbox| {
                for (rel, _) in &seeds {
                    let path = sb.root.join(rel);
                    let text = fs::read_to_string(&path).unwrap_or_default();
                    assert!(
                        text.contains("userOwnedKey") || text.contains("User prose"),
                        "{} ({}) {} lost what the user had in {}:\n{}",
                        target.id_str(),
                        label,
                        stage,
                        rel.display(),
                        text
                    );
                }
            };

            install_target_in(target, &loc, &opts("telemaco", false), &sb.home);
            check("install", &sb);
            install_target_in(target, &loc, &opts("/opt/moved/telemaco", false), &sb.home);
            check("reinstall", &sb);
            uninstall_target_in(target, &loc, &sb.home, false);
            check("uninstall", &sb);
        }
    }
}

#[test]
fn no_target_drops_a_key_the_user_added_to_our_entry() {
    // The env or the timeout a user sets on the telemaco server is theirs, and
    // a reinstall that rewrites the entry has to merge, not replace.
    for &target in TargetId::all() {
        for &(label, global) in LOCATIONS {
            let sb = Sandbox::new("userkeys");
            let loc = sb.location(global);
            install_target_in(target, &loc, &opts("telemaco", false), &sb.home);

            let mut seeded = Vec::new();
            for path in files_under(&sb.root) {
                let Ok(text) = fs::read_to_string(&path) else { continue };
                let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
                let mut touched = false;
                for container in ["mcpServers", "mcp"] {
                    if let Some(entry) = json
                        .get_mut(container)
                        .and_then(|c| c.get_mut("telemaco"))
                        .and_then(|e| e.as_object_mut())
                    {
                        entry.insert("userOwnedKey".to_string(), serde_json::json!("keep me"));
                        touched = true;
                    }
                }
                if touched {
                    fs::write(&path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
                    seeded.push(path);
                }
            }

            install_target_in(target, &loc, &opts("/opt/moved/telemaco", false), &sb.home);
            for path in &seeded {
                let text = fs::read_to_string(path).unwrap_or_default();
                assert!(
                    text.contains("userOwnedKey"),
                    "{} ({}) dropped a user key from {} on reinstall:\n{}",
                    target.id_str(),
                    label,
                    path.display(),
                    text
                );
            }
        }
    }
}

#[test]
fn every_target_follows_the_binary_when_it_moves() {
    // A hook that still names the old path fails silently: the agent runs a
    // binary that is not there and the user sees no web tools and no error.
    for &target in TargetId::all() {
        for &(label, global) in LOCATIONS {
            let sb = Sandbox::new("moved");
            let loc = sb.location(global);
            install_target_in(target, &loc, &opts("/old/place/telemaco", false), &sb.home);
            install_target_in(target, &loc, &opts("/new/place/telemaco", false), &sb.home);

            for path in files_under(&sb.root) {
                let Ok(text) = fs::read_to_string(&path) else { continue };
                assert!(
                    !text.contains("/old/place/telemaco"),
                    "{} ({}) left {} pointing at the old binary:\n{}",
                    target.id_str(),
                    label,
                    path.display(),
                    text
                );
            }
        }
    }
}

#[test]
fn every_target_keeps_the_line_endings_it_found() {
    // A config checked out with CRLF has to come back with CRLF. Rewriting it
    // to LF makes every line of the file show up in the user's next diff.
    for &target in TargetId::all() {
        for &(label, global) in LOCATIONS {
            let seeds: Vec<(PathBuf, String)> = paths_written(target, global)
                .into_iter()
                .filter_map(|rel| {
                    seed_for(&rel).map(|seed| (rel, seed.replace('\n', "\r\n")))
                })
                .collect();
            if seeds.is_empty() {
                continue;
            }

            let sb = Sandbox::new("crlf");
            let loc = sb.location(global);
            for (rel, seed) in &seeds {
                let path = sb.root.join(rel);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, seed).unwrap();
            }

            install_target_in(target, &loc, &opts("telemaco", false), &sb.home);

            for (rel, _) in &seeds {
                let path = sb.root.join(rel);
                let Ok(text) = fs::read_to_string(&path) else { continue };
                let lone_lf = text
                    .char_indices()
                    .filter(|(i, c)| *c == '\n' && (*i == 0 || text.as_bytes()[i - 1] != b'\r'))
                    .count();
                assert_eq!(
                    lone_lf,
                    0,
                    "{} ({}) rewrote {} with LF endings:\n{:?}",
                    target.id_str(),
                    label,
                    rel.display(),
                    text
                );
            }
        }
    }
}

#[test]
fn no_target_removes_a_neighbour_on_the_way_out() {
    // Another MCP server in the same file, and a hook that is not ours, are
    // the user's. Uninstall takes our entry out and stops there.
    for &target in TargetId::all() {
        for &(label, global) in LOCATIONS {
            let sb = Sandbox::new("neighbour");
            let loc = sb.location(global);
            install_target_in(target, &loc, &opts("telemaco", false), &sb.home);

            let mut seeded = Vec::new();
            for path in files_under(&sb.root) {
                let Ok(text) = fs::read_to_string(&path) else { continue };
                let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
                let mut touched = false;
                for container in ["mcpServers", "mcp"] {
                    if let Some(servers) = json.get_mut(container).and_then(|c| c.as_object_mut()) {
                        servers.insert(
                            "neighbour".to_string(),
                            serde_json::json!({"command": "their-server"}),
                        );
                        touched = true;
                    }
                }
                if touched {
                    fs::write(&path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
                    seeded.push(path);
                }
            }

            uninstall_target_in(target, &loc, &sb.home, false);
            for path in &seeded {
                let text = fs::read_to_string(path).unwrap_or_default();
                assert!(
                    text.contains("their-server"),
                    "{} ({}) took the user's other server with it out of {}:\n{}",
                    target.id_str(),
                    label,
                    path.display(),
                    text
                );
            }
        }
    }
}
