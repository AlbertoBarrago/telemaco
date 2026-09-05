use std::collections::HashSet;
use std::path::{Path, PathBuf};
use serde_json::{json, Value};

pub use crate::installer::instructions::{
    get_instructions_block, plan_marked_section, plan_remove_marked_section,
    remove_marked_section, replace_or_append_marked_section, Action,
};
pub use crate::installer::json_utils::{
    atomic_write_file, backup_file, backup_path, json_deep_equal, read_json_file,
    read_json_for_update, write_json_file, JsonSource,
};
pub use crate::installer::text_utils::with_line_ending;
pub use crate::installer::yaml_utils::{
    block_extent, indent_block, indent_of, prune_empty_yaml_keys, refresh_yaml_entry,
    reindent_block, remove_yaml_block, upsert_yaml_path, yaml_mcp_entry,
};
pub use crate::installer::toml_utils::{
    remove_toml_table, remove_toml_table_key, remove_top_level_key_with_value,
    toml_table_key_value, upsert_toml_table_keys,
};

pub use super::{
    DetectionResult, FileResult, Location, TargetId, TargetInstallOptions, TargetResult,
};

/// Reads an agent's documented home-directory override (`CODEX_HOME`,
/// `CLAUDE_CONFIG_DIR`, ...).
///
/// In a test build this reads a `TELEMACO_TEST_`-prefixed name instead of the
/// real one, so a test's explicit `home: &PathBuf` (a `TempDir`) can never be
/// silently overridden by whatever the ambient dev/CI shell happens to have
/// exported for the real variable - the bug that let a `cargo test` run with
/// a real `CLAUDE_CONFIG_DIR` set touch the developer's actual Claude Code
/// config instead of the isolated sandbox. A test that wants to exercise the
/// override behavior sets the fake `TELEMACO_TEST_*` name itself; production
/// builds never see it and always read the documented real name.
pub fn home_env_var(name: &str) -> Option<PathBuf> {
    #[cfg(test)]
    let value = std::env::var_os(format!("TELEMACO_TEST_{name}"));
    #[cfg(not(test))]
    let value = std::env::var_os(name);

    value.map(PathBuf::from).filter(|p| !p.as_os_str().is_empty())
}

/// What a target changed, plus anything the user has to be told: a config left
/// untouched because it could not be parsed, a write that failed, a rewrite
/// that dropped comments. Every helper records into one of these instead of
/// swallowing the error, so the installer never reports success for a file it
/// did not actually write.
pub struct Outcome {
    files: Vec<FileResult>,
    notes: Vec<String>,
    /// Report what would change without touching the disk.
    dry_run: bool,
    /// Configs already copied aside in this run, so each is backed up once.
    backed_up: HashSet<PathBuf>,
}

impl Outcome {
    pub fn new(dry_run: bool) -> Self {
        Outcome {
            files: Vec::new(),
            notes: Vec::new(),
            dry_run,
            backed_up: HashSet::new(),
        }
    }

    /// Copies a config aside the first time this run is about to rewrite it.
    ///
    /// An existing backup is never overwritten: it holds the file as it was
    /// before Telemaco first touched it, which is the state worth keeping.
    /// Returns whether a backup of `path` exists once this is done, so a note
    /// never points the user at a file that was never written.
    fn backup(&mut self, path: &Path) -> bool {
        if self.dry_run || !path.exists() {
            return false;
        }
        let backup = backup_path(path);
        if !self.backed_up.insert(path.to_path_buf()) || backup.exists() {
            return backup.exists();
        }
        // A file that already names telemaco is one we wrote on an earlier run,
        // so it is not the pre-Telemaco state worth keeping. Without this every
        // reinstall left another useless .telemaco-backup behind.
        if std::fs::read_to_string(path).map_or(false, |c| c.contains("telemaco")) {
            return false;
        }
        match backup_file(path) {
            Ok(_) => true,
            Err(e) => {
                self.note(e);
                false
            }
        }
    }

    pub fn push(&mut self, file: FileResult) {
        self.files.push(file);
    }

    pub fn note(&mut self, note: impl Into<String>) {
        self.notes.push(note.into());
    }

    /// Loads a JSON config that is about to be modified. `None` means the file
    /// must be left alone; the reason is already recorded as a note.
    ///
    /// A file that only parses as JSONC is backed up here, because writing it
    /// back as strict JSON drops its comments.
    pub fn load_json(&mut self, path: &Path) -> Option<Value> {
        match read_json_for_update(path) {
            Ok(cfg) => {
                if cfg.source == JsonSource::Jsonc {
                    let backed_up = self.backup(path);
                    if self.dry_run {
                        self.note(format!(
                            "{} uses JSON with comments; rewriting it would drop them.",
                            path.display()
                        ));
                    } else if backed_up {
                        self.note(format!(
                            "{} uses JSON with comments; they are dropped when rewriting it. Backup: {}",
                            path.display(),
                            backup_path(path).display()
                        ));
                    } else {
                        self.note(format!(
                            "{} uses JSON with comments; they are dropped when rewriting it.",
                            path.display()
                        ));
                    }
                }
                Some(cfg.value)
            }
            Err(e) => {
                self.note(e);
                None
            }
        }
    }

    /// Loads a text config that is about to be modified. `None` means the file
    /// exists but could not be read, so it must be left alone rather than
    /// replaced with a fresh one.
    pub fn load_text(&mut self, path: &Path) -> Option<String> {
        if !path.exists() {
            return Some(String::new());
        }
        match std::fs::read_to_string(path) {
            Ok(text) => Some(text),
            Err(e) => {
                self.note(format!("Could not read {}: {}", path.display(), e));
                None
            }
        }
    }

    /// Writes a JSON config, recording the file on success and the reason on
    /// failure.
    pub fn write_json(&mut self, path: &Path, value: &Value, action: Action) {
        if self.dry_run {
            self.push(FileResult { path: path.to_path_buf(), action });
            return;
        }
        let _ = self.backup(path);
        match write_json_file(path, value) {
            Ok(()) => self.push(FileResult { path: path.to_path_buf(), action }),
            Err(e) => self.note(e),
        }
    }

    /// Writes a text config atomically, recording the file on success and the
    /// reason on failure.
    pub fn write_text(&mut self, path: &Path, content: &str, action: Action) {
        if self.dry_run {
            self.push(FileResult { path: path.to_path_buf(), action });
            return;
        }
        let _ = self.backup(path);
        match atomic_write_file(path, content) {
            Ok(()) => self.push(FileResult { path: path.to_path_buf(), action }),
            Err(e) => self.note(format!("Could not write {}: {}", path.display(), e)),
        }
    }

    /// Deletes a config that no longer holds anything of the user's.
    ///
    /// A symlink is emptied instead of unlinked: the link is part of the user's
    /// dotfiles setup, and so is the file behind it, even once the only thing
    /// left in it was ours. `remove_file` is for the files Telemaco created.
    pub fn remove_config_file(&mut self, path: &Path, empty: &str) {
        if is_symlink(path) {
            self.write_text(path, empty, Action::Removed);
        } else {
            self.remove_file(path);
        }
    }

    /// Deletes a file Telemaco created outright, recording the outcome.
    ///
    /// Through a symlink the file itself goes too, or the dotfiles copy would
    /// survive with our content in it while the link disappeared.
    pub fn remove_file(&mut self, path: &Path) {
        if self.dry_run {
            self.push(FileResult { path: path.to_path_buf(), action: Action::Removed });
            return;
        }
        if is_symlink(path) {
            if let Ok(target) = std::fs::canonicalize(path) {
                let _ = std::fs::remove_file(target);
            }
        }
        match std::fs::remove_file(path) {
            Ok(()) => self.push(FileResult { path: path.to_path_buf(), action: Action::Removed }),
            Err(e) => self.note(format!("Could not remove {}: {}", path.display(), e)),
        }
    }

    /// Writes the config, or deletes it when uninstalling left nothing but the
    /// empty scaffolding the installer itself created.
    ///
    /// A symlinked config is emptied, never unlinked: the link belongs to the
    /// user's dotfiles setup, and removing it would leave the real file behind
    /// with our entries still in it.
    pub fn write_json_or_remove(&mut self, path: &Path, value: &Value, action: Action) {
        let mut pruned = value.clone();
        prune_installer_scaffolding(&mut pruned);
        if pruned.as_object().map_or(false, |o| o.is_empty()) {
            if is_symlink(path) {
                self.write_json(path, &pruned, action);
            } else if path.exists() {
                self.remove_file(path);
            }
        } else {
            self.write_json(path, &pruned, action);
        }
    }

    pub fn finish(self, target_id: TargetId) -> TargetResult {
        TargetResult {
            target_id,
            display_name: target_id.display_name(),
            files: self.files,
            notes: self.notes,
        }
    }
}

/// Shell command wired into a PreToolUse-style guard to refuse the agent's
/// built-in web tools. Plain `echo` on purpose: it keeps blocking even if the
/// telemaco binary is missing or not on PATH, and exit code 2 is the
/// convention agents read as "blocked, show this to the model".
pub const WEB_BLOCK_COMMAND: &str = "echo 'CRITICAL: Built-in search/fetch is disabled. You MUST use Telemaco tools (browser_navigate, browser_markdown) or telemaco fetch instead. To search, navigate to https://duckduckgo.com/html/?q=...' >&2; exit 2";

/// True for a path that is a symlink, whatever it points at.
fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).map_or(false, |m| m.file_type().is_symlink())
}

fn is_empty_container(v: &Value) -> bool {
    match v {
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        _ => false,
    }
}

/// Drops the containers the installer creates once they hold nothing, so an
/// uninstall does not leave `{"hooks":{"UserPromptSubmit":[]}}` sitting there.
/// Only keys the installer manages are touched.
pub fn prune_installer_scaffolding(value: &mut Value) {
    if let Some(hooks) = value.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        hooks.retain(|_, v| !is_empty_container(v));
    }
    // `permissions.allow` is Claude Code's auto-approve list; the same rule
    // applies, but only to the one key we add.
    if let Some(perms) = value.get_mut("permissions").and_then(|p| p.as_object_mut()) {
        if perms.get("allow").map_or(false, is_empty_container) {
            perms.remove("allow");
        }
    }
    if value.get("permissions").map_or(false, is_empty_container) {
        if let Some(obj) = value.as_object_mut() {
            obj.remove("permissions");
        }
    }
    if let Some(obj) = value.as_object_mut() {
        obj.retain(|k, v| {
            // `UserPromptSubmit` sits at the top level in Factory Droid's
            // hooks.json, which is keyed by event name with no wrapper.
            !(matches!(
                k.as_str(),
                "hooks"
                    | "autoApprove"
                    | "mcpServers"
                    | "mcp"
                    | "UserPromptSubmit"
                    | "enabledMcpjsonServers"
            ) && is_empty_container(v))
        });
    }
}

/// Returns standard stdio MCP arguments depending on stealth flag.
/// The context file names an agent has been told to load, if any.
///
/// Gemini CLI and Qwen Code share this setting: `context.fileName` replaces the
/// default context file, taking either one name or a list of names that are all
/// loaded and concatenated, and older settings files carry the same thing as a
/// flat `contextFileName`. `None` means nothing was configured and the agent's
/// documented default is still the file to write.
///
/// `settings_files` is searched in order, so the caller puts the project's
/// settings before the user's when a project layer can override.
pub fn configured_context_file_names(settings_files: &[PathBuf]) -> Option<Vec<String>> {
    for path in settings_files {
        if !path.exists() {
            continue;
        }
        let json = read_json_file(path);
        let configured = json
            .get("context")
            .and_then(|c| c.get("fileName"))
            .or_else(|| json.get("contextFileName"));
        let names: Vec<String> = match configured {
            Some(Value::String(one)) => vec![one.trim().to_string()],
            Some(Value::Array(many)) => many
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .collect(),
            _ => Vec::new(),
        };
        let names: Vec<String> = names.into_iter().filter(|n| !n.is_empty()).collect();
        if !names.is_empty() {
            return Some(names);
        }
    }
    None
}

/// The instructions file to write in `dir`, honouring `AGENTS.override.md`.
///
/// Codex and Pi share this rule: "If a directory contains `AGENTS.override.md`,
/// Pi loads it instead of `AGENTS.md` or `CLAUDE.md` from that directory"
/// (earendil-works/pi packages/coding-agent/README.md), and Codex reads the
/// same file in place of `AGENTS.md`. Where one exists, `AGENTS.md` is the file
/// nobody reads.
pub fn agents_file_in(dir: &Path) -> PathBuf {
    let override_path = dir.join("AGENTS.override.md");
    if override_path.exists() {
        override_path
    } else {
        dir.join("AGENTS.md")
    }
}

/// Both names, for cleanup: the override may have appeared or gone between
/// install and uninstall.
pub fn all_agents_files_in(dir: &Path) -> Vec<PathBuf> {
    vec![dir.join("AGENTS.override.md"), dir.join("AGENTS.md")]
}

pub fn stdio_mcp_args(stealth: bool) -> Vec<String> {
    if stealth {
        vec!["mcp".to_string(), "--stealth".to_string()]
    } else {
        vec!["mcp".to_string()]
    }
}

/// Constructs a standard stdio MCP JSON server definition.
pub fn stdio_mcp_entry(binary_path: &str, stealth: bool) -> Value {
    json!({
        "command": binary_path,
        "args": stdio_mcp_args(stealth),
    })
}

/// Same as `stdio_mcp_entry` plus the explicit `"type": "stdio"` that Claude
/// Code and Factory Droid expect.
pub fn stdio_typed_mcp_entry(binary_path: &str, stealth: bool) -> Value {
    json!({
        "type": "stdio",
        "command": binary_path,
        "args": stdio_mcp_args(stealth),
    })
}

/// Shell command an agent runs for the prompt hook.
///
/// It has to use the same resolved binary as the MCP entry: GUI-launched agents
/// (Cursor, Windsurf, Antigravity, Kiro) inherit a minimal PATH, so a bare
/// `telemaco` would silently fail to start while the MCP server still worked.
pub fn prompt_hook_command(binary_path: &str) -> String {
    if binary_path.contains(char::is_whitespace) {
        format!("\"{}\" prompt-hook", binary_path)
    } else {
        format!("{} prompt-hook", binary_path)
    }
}

/// Same, for an agent that parses the hook's stdout as JSON instead of taking
/// it as plain text (Qwen Code).
pub fn prompt_hook_command_json(binary_path: &str) -> String {
    format!("{} --format json", prompt_hook_command(binary_path))
}

/// The prompt-hook command for Cursor's `sessionStart`, which reads
/// `additional_context` from the hook's JSON stdout.
pub fn prompt_hook_command_cursor(binary_path: &str) -> String {
    format!("{} --format cursor", prompt_hook_command(binary_path))
}

/// Poolside parses any non-empty stdout as a decision object, so the answer has
/// to be its own snake_case shape or the hook counts as failed.
pub fn prompt_hook_command_poolside(binary_path: &str) -> String {
    format!("{} --format poolside", prompt_hook_command(binary_path))
}

/// Recognises our own prompt hook whatever binary path it was installed with,
/// so reinstalling from a different path updates the hook instead of appending
/// a second copy.
pub fn is_telemaco_hook_command(cmd: &str) -> bool {
    cmd.contains("prompt-hook") && cmd.contains("telemaco")
}

/// True when a config file already carries a telemaco prompt hook, tested on
/// the raw text for detection.
pub fn text_has_telemaco_hook(content: &str) -> bool {
    content.contains("prompt-hook") && content.contains("telemaco")
}

fn group_has_telemaco_hook(group: &Value) -> bool {
    group
        .get("hooks")
        .and_then(|h| h.as_array())
        .map_or(false, |arr| {
            arr.iter().any(|entry| {
                entry
                    .get("command")
                    .and_then(|c| c.as_str())
                    .map_or(false, is_telemaco_hook_command)
            })
        })
}

/// Injects a `UserPromptSubmit` command hook into a JSON structure (Claude,
/// Codex, Droid, Cursor, Qwen, DeepSeek format). Returns whether the structure
/// changed: an existing telemaco hook is rewritten in place when the command
/// differs, so a reinstall from a new binary path stays idempotent.
pub fn add_user_prompt_hook(hooks_json: &mut Value, cmd: &str) -> bool {
    add_prompt_hook_for_event(hooks_json, "UserPromptSubmit", cmd)
}

/// Same, for an agent that calls the prompt event something else. Gemini CLI's
/// is `BeforeAgent`, "after a user submits a prompt, but before the agent
/// begins planning" (google-gemini/gemini-cli docs/hooks/reference.md).
pub fn add_prompt_hook_for_event(hooks_json: &mut Value, event: &str, cmd: &str) -> bool {
    let Some(root) = hooks_json.as_object_mut() else {
        return false;
    };
    let hooks_obj = root.entry("hooks").or_insert_with(|| json!({}));
    let Some(hooks_map) = hooks_obj.as_object_mut() else {
        return false;
    };
    upsert_prompt_hook_in_event_map(hooks_map, event, cmd)
}

/// Removal counterpart of `add_prompt_hook_for_event`.
pub fn remove_prompt_hook_for_event(hooks_json: &mut Value, event: &str) -> bool {
    match hooks_json.get_mut("hooks").and_then(|v| v.as_object_mut()) {
        Some(map) => remove_prompt_hook_from_event_map(map, event),
        None => false,
    }
}

/// Same, for a config file that is keyed directly by event name with no
/// wrapping `hooks` key.
///
/// Factory Droid's `hooks.json` is written that way: "Standalone hooks.json
/// files are keyed directly by event name" (docs.factory.ai/harness/hooks).
/// The `hooks` wrapper only applies inside `settings.json`, so the wrapped file
/// we used to write was simply ignored.
pub fn add_user_prompt_hook_flat(hooks_json: &mut Value, cmd: &str) -> bool {
    let Some(map) = hooks_json.as_object_mut() else {
        return false;
    };
    upsert_prompt_hook_in_event_map(map, "UserPromptSubmit", cmd)
}

fn upsert_prompt_hook_in_event_map(
    hooks_map: &mut serde_json::Map<String, Value>,
    event: &str,
    cmd: &str,
) -> bool {
    let ups = hooks_map.entry(event).or_insert_with(|| json!([]));
    let Some(groups) = ups.as_array_mut() else {
        return false;
    };

    // Every group of ours, not just the first: an older version could leave a
    // second one behind, and a stale copy points at a binary that has moved.
    let mut found = false;
    let mut changed = false;
    for group in groups.iter_mut() {
        if !group_has_telemaco_hook(group) {
            continue;
        }
        found = true;
        if let Some(arr) = group.get_mut("hooks").and_then(|h| h.as_array_mut()) {
            for entry in arr.iter_mut() {
                let is_ours = entry
                    .get("command")
                    .and_then(|c| c.as_str())
                    .map_or(false, is_telemaco_hook_command);
                if is_ours && entry.get("command").and_then(|c| c.as_str()) != Some(cmd) {
                    entry["command"] = json!(cmd);
                    changed = true;
                }
            }
        }
    }
    if found {
        return changed;
    }

    groups.push(json!({
        "hooks": [
            {
                "type": "command",
                "command": cmd
            }
        ]
    }));
    true
}

/// Removes our `UserPromptSubmit` command hook from a JSON structure.
pub fn remove_user_prompt_hook(hooks_json: &mut Value) -> bool {
    match hooks_json.get_mut("hooks").and_then(|v| v.as_object_mut()) {
        Some(map) => remove_prompt_hook_from_event_map(map, "UserPromptSubmit"),
        None => false,
    }
}

/// Removal counterpart of `add_user_prompt_hook_flat`.
pub fn remove_user_prompt_hook_flat(hooks_json: &mut Value) -> bool {
    match hooks_json.as_object_mut() {
        Some(map) => remove_prompt_hook_from_event_map(map, "UserPromptSubmit"),
        None => false,
    }
}

fn remove_prompt_hook_from_event_map(
    hooks_map: &mut serde_json::Map<String, Value>,
    event: &str,
) -> bool {
    let Some(ups) = hooks_map.get_mut(event).and_then(|v| v.as_array_mut()) else {
        return false;
    };
    let prev_len = ups.len();
    ups.retain(|g| !group_has_telemaco_hook(g));
    ups.len() != prev_len
}

/// Upserts a stdio MCP server entry into a JSON file with an `mcpServers`
/// object.
pub fn upsert_mcp_server(out: &mut Outcome, file_path: &Path, server_name: &str, target_entry: Value) {
    upsert_named_server(out, file_path, "mcpServers", server_name, target_entry)
}

/// Upserts a server entry under an arbitrary top-level key (`mcpServers`,
/// `mcp`, ...).
pub fn upsert_named_server(
    out: &mut Outcome,
    file_path: &Path,
    container: &str,
    server_name: &str,
    target_entry: Value,
) {
    let existed = file_path.exists();
    let Some(mut json) = out.load_json(file_path) else {
        return;
    };

    match upsert_server_entry(&mut json, container, server_name, target_entry) {
        Ok(true) => {
            let action = if existed { Action::Updated } else { Action::Created };
            out.write_json(file_path, &json, action);
        }
        Ok(false) => {
            out.push(FileResult { path: file_path.to_path_buf(), action: Action::Unchanged })
        }
        Err(why) => out.note(format!("{}: {}", file_path.display(), why)),
    }
}

/// Puts a server entry into an already-loaded config, returning whether it
/// changed. Split out of `upsert_named_server` so a target whose MCP entry and
/// hook live in the same file can write it once instead of twice.
pub fn upsert_server_entry(
    json: &mut Value,
    container: &str,
    server_name: &str,
    target_entry: Value,
) -> Result<bool, String> {
    let servers = json
        .as_object_mut()
        .expect("load_json guarantees an object")
        .entry(container)
        .or_insert_with(|| json!({}));
    let Some(servers_obj) = servers.as_object_mut() else {
        return Err(format!("'{}' is not an object; leaving it alone", container));
    };

    // Merge, do not replace: anything the user added to our entry (`env` with a
    // corporate proxy, `timeout`, ...) has to survive a reinstall. Only the
    // keys we manage are overwritten.
    let merged = match (servers_obj.get(server_name), target_entry.as_object()) {
        (Some(Value::Object(existing)), Some(ours)) => {
            let mut m = existing.clone();
            for (k, v) in ours {
                m.insert(k.clone(), v.clone());
            }
            Value::Object(m)
        }
        _ => target_entry,
    };

    if servers_obj.get(server_name).map_or(false, |cur| json_deep_equal(cur, &merged)) {
        return Ok(false);
    }
    servers_obj.insert(server_name.to_string(), merged);
    Ok(true)
}

/// Removes an MCP server from a JSON file with an `mcpServers` object.
pub fn remove_mcp_server(out: &mut Outcome, file_path: &Path, server_name: &str) {
    remove_named_server(out, file_path, "mcpServers", server_name)
}

/// Removes a server entry from under an arbitrary top-level key.
pub fn remove_named_server(out: &mut Outcome, file_path: &Path, container: &str, server_name: &str) {
    if !file_path.exists() {
        return;
    }
    let Some(mut json) = out.load_json(file_path) else {
        return;
    };
    let removed = json
        .get_mut(container)
        .and_then(|v| v.as_object_mut())
        .map_or(false, |servers| servers.remove(server_name).is_some());
    if removed {
        // Drops the file when nothing but the empty container we created is
        // left, so uninstalling does not litter `{"mcpServers": {}}` about.
        out.write_json_or_remove(file_path, &json, Action::Removed);
    }
}

/// Writes a dedicated rule file: frontmatter the editor reads, then our block.
///
/// Unlike `update_instructions` the file is ours alone, so it is replaced
/// wholesale rather than merged; `remove_instructions` still takes the block
/// back out and drops the file once nothing is left.
pub fn update_rule_file(out: &mut Outcome, path: &Path, frontmatter: &str, stealth: bool) {
    let content = format!("---\n{}\n---\n\n{}\n", frontmatter, get_instructions_block(stealth));
    let existed = path.exists();
    if existed && std::fs::read_to_string(path).ok().as_deref() == Some(content.as_str()) {
        out.push(FileResult { path: path.to_path_buf(), action: Action::Unchanged });
        return;
    }
    let action = if existed { Action::Updated } else { Action::Created };
    out.write_text(path, &content, action);
}

/// Updates the Telemaco block in an instructions markdown file.
pub fn update_instructions(out: &mut Outcome, path: &Path, stealth: bool) {
    let block = get_instructions_block(stealth);
    let result = if out.dry_run {
        plan_marked_section(path, &block)
    } else {
        let _ = out.backup(path);
        replace_or_append_marked_section(path, &block)
    };
    match result {
        Ok(action) => out.push(FileResult { path: path.to_path_buf(), action }),
        Err(e) => out.note(format!(
            "Failed to update instructions in {}: {}",
            path.display(),
            e
        )),
    }
}

/// Removes the Telemaco block from an instructions markdown file.
pub fn remove_instructions(out: &mut Outcome, path: &Path) {
    let result = if out.dry_run {
        plan_remove_marked_section(path)
    } else {
        remove_marked_section(path)
    };
    match result {
        Ok(Action::Removed) => {
            out.push(FileResult { path: path.to_path_buf(), action: Action::Removed })
        }
        Ok(_) => {}
        Err(e) => out.note(format!(
            "Failed to remove instructions from {}: {}",
            path.display(),
            e
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_hook_command_uses_resolved_binary() {
        assert_eq!(
            prompt_hook_command("/opt/bin/telemaco"),
            "/opt/bin/telemaco prompt-hook"
        );
        assert_eq!(
            prompt_hook_command("/My Apps/telemaco"),
            "\"/My Apps/telemaco\" prompt-hook"
        );
    }

    #[test]
    fn test_add_hook_is_idempotent_across_binary_paths() {
        let mut cfg = json!({});
        assert!(add_user_prompt_hook(&mut cfg, "telemaco prompt-hook"));
        // Same command again: no change, no second entry.
        assert!(!add_user_prompt_hook(&mut cfg, "telemaco prompt-hook"));
        // Reinstall from an absolute path rewrites in place.
        assert!(add_user_prompt_hook(&mut cfg, "/opt/bin/telemaco prompt-hook"));

        let groups = cfg["hooks"]["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["hooks"][0]["command"], "/opt/bin/telemaco prompt-hook");

        assert!(remove_user_prompt_hook(&mut cfg));
        assert!(cfg["hooks"]["UserPromptSubmit"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_remove_hook_leaves_foreign_hooks_alone() {
        let mut cfg = json!({
            "hooks": {
                "UserPromptSubmit": [
                    {"hooks": [{"type": "command", "command": "other-tool run"}]}
                ]
            }
        });
        assert!(add_user_prompt_hook(&mut cfg, "/opt/bin/telemaco prompt-hook"));
        assert!(remove_user_prompt_hook(&mut cfg));
        let groups = cfg["hooks"]["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["hooks"][0]["command"], "other-tool run");
    }
}
