use std::path::{Path, PathBuf};

pub mod common;
#[cfg(test)]
mod invariants;
pub mod claude;
pub mod cursor;
pub mod codex;
pub mod gemini;
pub mod antigravity;
pub mod windsurf;
pub mod opencode;
pub mod roocline;
pub mod pi;
pub mod deepseek;
pub mod qwen;
pub mod droid;
pub mod poolside;
pub mod kiro;
pub mod hermes;

use crate::installer::instructions::Action;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    Global,
    Folder(PathBuf),
}

impl Location {
    pub fn is_global(&self) -> bool {
        matches!(self, Location::Global)
    }

    pub fn folder(&self) -> Option<&Path> {
        match self {
            Location::Global => None,
            Location::Folder(p) => Some(p.as_path()),
        }
    }
}

impl std::fmt::Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Location::Global => write!(f, "global"),
            Location::Folder(p) => write!(f, "folder ({})", p.display()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum LocationArg {
    Global,
    Local,
}

impl std::fmt::Display for LocationArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LocationArg::Global => write!(f, "global"),
            LocationArg::Local => write!(f, "local"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetId {
    Claude,
    Cursor,
    Codex,
    Gemini,
    Antigravity,
    Windsurf,
    OpenCode,
    RooCline,
    Pi,
    DeepSeek,
    QwenCode,
    Droid,
    Poolside,
    Kiro,
    Hermes,
}

impl TargetId {
    pub fn all() -> &'static [TargetId] {
        &[
            TargetId::Claude,
            TargetId::Cursor,
            TargetId::Codex,
            TargetId::Gemini,
            TargetId::Antigravity,
            TargetId::Windsurf,
            TargetId::OpenCode,
            TargetId::RooCline,
            TargetId::Pi,
            TargetId::DeepSeek,
            TargetId::QwenCode,
            TargetId::Droid,
            TargetId::Poolside,
            TargetId::Kiro,
            TargetId::Hermes,
        ]
    }

    pub fn id_str(&self) -> &'static str {
        match self {
            TargetId::Claude => "claude",
            TargetId::Cursor => "cursor",
            TargetId::Codex => "codex",
            TargetId::Gemini => "gemini",
            TargetId::Antigravity => "antigravity",
            TargetId::Windsurf => "windsurf",
            TargetId::OpenCode => "opencode",
            TargetId::RooCline => "roo-cline",
            TargetId::Pi => "pi",
            TargetId::DeepSeek => "deepseek",
            TargetId::QwenCode => "qwen",
            TargetId::Droid => "droid",
            TargetId::Poolside => "poolside",
            TargetId::Kiro => "kiro",
            TargetId::Hermes => "hermes",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            TargetId::Claude => "Claude Code",
            TargetId::Cursor => "Cursor",
            TargetId::Codex => "OpenAI Codex CLI",
            TargetId::Gemini => "Gemini CLI",
            TargetId::Antigravity => "Google Antigravity (IDE and CLI)",
            TargetId::Windsurf => "Codeium Windsurf",
            TargetId::OpenCode => "OpenCode",
            TargetId::RooCline => "Roo Code / Cline",
            TargetId::Pi => "Pi Coding Agent",
            TargetId::DeepSeek => "DeepSeek Harness",
            TargetId::QwenCode => "Qwen Code",
            TargetId::Droid => "Factory Droid",
            TargetId::Poolside => "Poolside Agent",
            TargetId::Kiro => "Kiro",
            TargetId::Hermes => "Hermes Agent",
        }
    }

    pub fn parse(s: &str) -> Option<TargetId> {
        match s.to_ascii_lowercase().replace('_', "-").as_str() {
            "claude" | "claude-code" => Some(TargetId::Claude),
            "cursor" => Some(TargetId::Cursor),
            "codex" | "openai-codex" => Some(TargetId::Codex),
            "gemini" | "gemini-cli" => Some(TargetId::Gemini),
            "antigravity" | "antigravity-ide" | "agy" => Some(TargetId::Antigravity),
            "windsurf" => Some(TargetId::Windsurf),
            "opencode" => Some(TargetId::OpenCode),
            "roo" | "cline" | "roo-cline" | "roocline" => Some(TargetId::RooCline),
            "pi" | "pi-agent" | "pi-dev" => Some(TargetId::Pi),
            "deepseek" | "dsh" | "deepseek-harness" => Some(TargetId::DeepSeek),
            "qwen" | "qwen-code" | "qwencode" => Some(TargetId::QwenCode),
            "droid" | "factory" | "factory-droid" => Some(TargetId::Droid),
            "poolside" | "pool" => Some(TargetId::Poolside),
            "kiro" | "kiro-cli" | "kiro-ide" | "kiro-dev" => Some(TargetId::Kiro),
            "hermes" | "hermes-agent" | "nous" => Some(TargetId::Hermes),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn supports_location(&self, _loc: &Location) -> bool {
        true
    }
}

pub struct DetectionResult {
    pub installed: bool,
    pub already_configured: bool,
    pub config_path: Option<PathBuf>,
    pub hint: String,
}

pub struct FileResult {
    pub path: PathBuf,
    pub action: Action,
}

pub struct TargetResult {
    #[allow(dead_code)]
    pub target_id: TargetId,
    pub display_name: &'static str,
    pub files: Vec<FileResult>,
    pub notes: Vec<String>,
}

pub struct TargetInstallOptions {
    pub auto_allow: bool,
    pub stealth: bool,
    pub binary_path: String,
    /// Refuse the agent's own web tools so it has to go through Telemaco.
    pub block_builtin_web: bool,
    /// Report what would change without touching the disk.
    pub dry_run: bool,
}

pub fn get_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .and_then(|h| if h.is_empty() { None } else { Some(PathBuf::from(h)) })
        .or_else(|| {
            #[cfg(windows)]
            {
                std::env::var_os("USERPROFILE").map(PathBuf::from)
            }
            #[cfg(not(windows))]
            {
                None
            }
        })
}

pub fn tildify(p: &Path) -> String {
    if let Some(home) = get_home_dir() {
        if let Ok(rel) = p.strip_prefix(&home) {
            return format!("~/{}", rel.display());
        }
    }
    p.display().to_string()
}

/// Absolute path to write into agent configs.
///
/// The running binary wins. Preferring `~/.local/bin/telemaco` instead meant a
/// stale copy there got wired in while a newer binary was doing the install,
/// and hooks from a version that lacks `prompt-hook` fail silently.
pub fn resolve_telemaco_binary() -> String {
    if let Ok(cur_exe) = std::env::current_exe() {
        if cur_exe.exists() {
            return cur_exe.display().to_string();
        }
    }

    if let Some(home) = get_home_dir() {
        let local_bin = home.join(".local").join("bin").join("telemaco");
        if local_bin.exists() {
            return local_bin.display().to_string();
        }
        let cargo_bin = home.join(".cargo").join("bin").join("telemaco");
        if cargo_bin.exists() {
            return cargo_bin.display().to_string();
        }
    }

    "telemaco".to_string()
}

/// The MCP config snippet for a target, built from the very entry builders the
/// installer writes. Printing and installing cannot drift apart.
pub fn config_snippet(target: TargetId, binary: &str, stealth: bool, auto_allow: bool) -> String {
    use common::{stdio_mcp_args, stdio_mcp_entry, stdio_typed_mcp_entry, yaml_mcp_entry};

    fn wrap(container: &str, entry: serde_json::Value) -> String {
        let doc = serde_json::json!({ container: { "telemaco": entry } });
        serde_json::to_string_pretty(&doc).unwrap_or_default()
    }

    match target {
        TargetId::Codex => {
            let args = stdio_mcp_args(stealth)
                .iter()
                .map(|a| format!("\"{}\"", a))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "[mcp_servers.telemaco]\ncommand = \"{}\"\nargs = [{}]",
                binary, args
            )
        }
        TargetId::Hermes => {
            let entry = yaml_mcp_entry("telemaco", binary, &stdio_mcp_args(stealth));
            let indented: Vec<String> = entry.lines().map(|l| format!("  {}", l)).collect();
            format!("mcp_servers:\n{}", indented.join("\n"))
        }
        TargetId::Poolside => {
            let entry = poolside::mcp_yaml_entry(binary, stealth);
            let indented: Vec<String> = entry.lines().map(|l| format!("  {}", l)).collect();
            format!("mcp_servers:\n{}", indented.join("\n"))
        }
        TargetId::OpenCode => wrap("mcp", opencode::mcp_entry(binary, stealth)),
        TargetId::RooCline => wrap("mcpServers", roocline::mcp_entry(binary, stealth)),
        TargetId::Kiro => wrap("mcpServers", kiro::mcp_entry(binary, stealth, auto_allow)),
        TargetId::Claude | TargetId::Droid => {
            wrap("mcpServers", stdio_typed_mcp_entry(binary, stealth))
        }
        TargetId::Pi | TargetId::DeepSeek => {
            "Telemaco is configured through AGENTS.md instructions only; this agent has no MCP config file.".to_string()
        }
        _ => wrap("mcpServers", stdio_mcp_entry(binary, stealth)),
    }
}

/// Detection, install and uninstall for one target, in one row.
///
/// These used to be three separate fourteen-arm matches. A target wired into
/// one of them and forgotten in another still compiled, which is how Windsurf
/// grew a `.devin/` write path that detection never looked at. One table means
/// a target is either fully wired or it does not build.
struct TargetOps {
    detect: fn(&Location, Option<&PathBuf>) -> DetectionResult,
    install: fn(&Location, &TargetInstallOptions, &PathBuf) -> TargetResult,
    uninstall: fn(&Location, &PathBuf, bool) -> TargetResult,
}

macro_rules! target_ops {
    ($m:ident) => {
        TargetOps {
            detect: $m::detect,
            install: $m::install,
            uninstall: $m::uninstall,
        }
    };
}

fn ops_for(target: TargetId) -> TargetOps {
    match target {
        TargetId::Claude => target_ops!(claude),
        TargetId::Cursor => target_ops!(cursor),
        TargetId::Codex => target_ops!(codex),
        TargetId::Gemini => target_ops!(gemini),
        TargetId::Antigravity => target_ops!(antigravity),
        TargetId::Windsurf => target_ops!(windsurf),
        TargetId::OpenCode => target_ops!(opencode),
        TargetId::RooCline => target_ops!(roocline),
        TargetId::Pi => target_ops!(pi),
        TargetId::DeepSeek => target_ops!(deepseek),
        TargetId::QwenCode => target_ops!(qwen),
        TargetId::Droid => target_ops!(droid),
        TargetId::Poolside => target_ops!(poolside),
        TargetId::Kiro => target_ops!(kiro),
        TargetId::Hermes => target_ops!(hermes),
    }
}

/// Convenience wrapper over `detect_target_in` for tests that do not care
/// which home is used. Production code resolves an explicit home (the real
/// one, or a `--folder` chosen as global) and calls `detect_target_in`
/// directly.
#[cfg(test)]
pub fn detect_target(target: TargetId, loc: &Location) -> DetectionResult {
    detect_target_in(target, loc, get_home_dir().as_ref())
}

/// Detection against an explicit home, so tests never read the real one.
pub fn detect_target_in(target: TargetId, loc: &Location, home: Option<&PathBuf>) -> DetectionResult {
    (ops_for(target).detect)(loc, home)
}

#[cfg(test)]
pub fn detect_folder_targets(folder: &Path) -> Vec<TargetId> {
    let loc = Location::Folder(folder.to_path_buf());
    let mut detected = Vec::new();
    for &target in TargetId::all() {
        let res = detect_target(target, &loc);
        if res.installed {
            detected.push(target);
        }
    }
    detected
}

/// Convenience wrapper over `install_target_in`, see `detect_target`.
#[cfg(test)]
pub fn install_target(
    target: TargetId,
    loc: &Location,
    opts: &TargetInstallOptions,
) -> TargetResult {
    let home = get_home_dir().unwrap_or_else(|| PathBuf::from("."));
    install_target_in(target, loc, opts, &home)
}

/// Install against an explicit home, so tests never write into the real one.
pub fn install_target_in(
    target: TargetId,
    loc: &Location,
    opts: &TargetInstallOptions,
    home: &PathBuf,
) -> TargetResult {
    (ops_for(target).install)(loc, opts, home)
}

/// Convenience wrapper over `uninstall_target_in`, see `detect_target`.
#[cfg(test)]
pub fn uninstall_target(target: TargetId, loc: &Location, dry_run: bool) -> TargetResult {
    let home = get_home_dir().unwrap_or_else(|| PathBuf::from("."));
    uninstall_target_in(target, loc, &home, dry_run)
}

/// Uninstall against an explicit home, so tests never touch the real one.
pub fn uninstall_target_in(
    target: TargetId,
    loc: &Location,
    home: &PathBuf,
    dry_run: bool,
) -> TargetResult {
    (ops_for(target).uninstall)(loc, home, dry_run)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use crate::installer::json_utils::read_json_file;

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(name: &str) -> Self {
            let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!("telemaco_test_{}_{}_{}", name, std::process::id(), id));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    /// Serializes every test that sets one of the fake `TELEMACO_TEST_*`
    /// home-dir overrides `home_env_var` reads in a test build, so two tests
    /// never race setting the same process-global variable, and restores
    /// whatever was there before (nothing, normally) once the test is done.
    static ENV_VAR_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct FakeHomeVar {
        key: String,
        prev: Option<std::ffi::OsString>,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl FakeHomeVar {
        /// Sets `TELEMACO_TEST_<name>`, the test-build stand-in for the real
        /// override `home_env_var` would read in production (`CODEX_HOME`,
        /// `CLAUDE_CONFIG_DIR`, ...). Never touches the real variable, so a
        /// real one exported in the ambient shell cannot leak into the test
        /// and cannot be disturbed by it either.
        fn set(name: &str, value: &Path) -> Self {
            let guard = ENV_VAR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let key = format!("TELEMACO_TEST_{name}");
            let prev = std::env::var_os(&key);
            std::env::set_var(&key, value);
            Self { key, prev, _guard: guard }
        }
    }

    impl Drop for FakeHomeVar {
        fn drop(&mut self) {
            match self.prev.take() {
                Some(v) => std::env::set_var(&self.key, v),
                None => std::env::remove_var(&self.key),
            }
        }
    }

    #[test]
    fn test_target_id_parsing() {
        assert_eq!(TargetId::parse("claude"), Some(TargetId::Claude));
        assert_eq!(TargetId::parse("claude-code"), Some(TargetId::Claude));
        assert_eq!(TargetId::parse("cursor"), Some(TargetId::Cursor));
        assert_eq!(TargetId::parse("codex"), Some(TargetId::Codex));
        assert_eq!(TargetId::parse("openai-codex"), Some(TargetId::Codex));
        assert_eq!(TargetId::parse("gemini"), Some(TargetId::Gemini));
        assert_eq!(TargetId::parse("antigravity"), Some(TargetId::Antigravity));
        assert_eq!(TargetId::parse("agy"), Some(TargetId::Antigravity));
        assert_eq!(TargetId::parse("windsurf"), Some(TargetId::Windsurf));
        assert_eq!(TargetId::parse("opencode"), Some(TargetId::OpenCode));
        assert_eq!(TargetId::parse("roo"), Some(TargetId::RooCline));
        assert_eq!(TargetId::parse("cline"), Some(TargetId::RooCline));
        assert_eq!(TargetId::parse("roo-cline"), Some(TargetId::RooCline));
        assert_eq!(TargetId::parse("pi"), Some(TargetId::Pi));
        assert_eq!(TargetId::parse("pi-dev"), Some(TargetId::Pi));
        assert_eq!(TargetId::parse("deepseek"), Some(TargetId::DeepSeek));
        assert_eq!(TargetId::parse("dsh"), Some(TargetId::DeepSeek));
        assert_eq!(TargetId::parse("qwen"), Some(TargetId::QwenCode));
        assert_eq!(TargetId::parse("qwen-code"), Some(TargetId::QwenCode));
        assert_eq!(TargetId::parse("droid"), Some(TargetId::Droid));
        assert_eq!(TargetId::parse("factory"), Some(TargetId::Droid));
        assert_eq!(TargetId::parse("poolside"), Some(TargetId::Poolside));
        assert_eq!(TargetId::parse("pool"), Some(TargetId::Poolside));
        assert_eq!(TargetId::parse("kiro"), Some(TargetId::Kiro));
        assert_eq!(TargetId::parse("kiro-cli"), Some(TargetId::Kiro));
        assert_eq!(TargetId::parse("unknown"), None);
    }

    #[test]
    fn test_folder_detection() {
        let temp = TempDir::new("detect");
        let path = temp.path();

        // Empty directory detects nothing
        let detected = detect_folder_targets(path);
        assert!(detected.is_empty());

        // Claude marker (.claude directory)
        fs::create_dir_all(path.join(".claude")).unwrap();
        let detected = detect_folder_targets(path);
        assert!(detected.contains(&TargetId::Claude));

        // Cursor marker (.cursorrules)
        fs::write(path.join(".cursorrules"), "").unwrap();
        let detected = detect_folder_targets(path);
        assert!(detected.contains(&TargetId::Cursor));

        // Windsurf marker (.windsurfrules)
        fs::write(path.join(".windsurfrules"), "").unwrap();
        let detected = detect_folder_targets(path);
        assert!(detected.contains(&TargetId::Windsurf));

        // RooCline marker (.roomodes)
        fs::write(path.join(".roomodes"), "").unwrap();
        let detected = detect_folder_targets(path);
        assert!(detected.contains(&TargetId::RooCline));

        // Kiro marker (.kiro directory)
        fs::create_dir_all(path.join(".kiro")).unwrap();
        let detected = detect_folder_targets(path);
        assert!(detected.contains(&TargetId::Kiro));
    }

    #[test]
    fn test_folder_install_and_uninstall_claude() {
        let temp = TempDir::new("claude_install");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        let opts = TargetInstallOptions {
            auto_allow: true,
            stealth: true,
            binary_path: "/bin/telemaco".to_string(),
            block_builtin_web: true,
            dry_run: false,
        };

        // Install
        let res = install_target(TargetId::Claude, &loc, &opts);
        assert_eq!(res.target_id, TargetId::Claude);
        assert!(path.join(".mcp.json").exists());
        assert!(path.join(".claude").join("settings.json").exists());
        assert!(path.join("CLAUDE.md").exists());

        // Verify .mcp.json contents
        let mcp = read_json_file(&path.join(".mcp.json"));
        assert!(mcp["mcpServers"]["telemaco"].is_object());

        // Verify detection sees it as already configured
        let det = detect_target(TargetId::Claude, &loc);
        assert!(det.installed);
        assert!(det.already_configured);

        // Uninstall
        let unres = uninstall_target(TargetId::Claude, &loc, false);
        assert_eq!(unres.target_id, TargetId::Claude);

        let mcp_after = read_json_file(&path.join(".mcp.json"));
        assert!(mcp_after["mcpServers"]["telemaco"].is_null());
    }

    #[test]
    fn test_folder_install_and_uninstall_cursor() {
        let temp = TempDir::new("cursor_install");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        let opts = TargetInstallOptions {
            auto_allow: true,
            stealth: false,
            binary_path: "telemaco".to_string(),
            block_builtin_web: true,
            dry_run: false,
        };

        install_target(TargetId::Cursor, &loc, &opts);
        assert!(path.join(".cursor").join("mcp.json").exists());
        assert!(path.join(".cursor").join("rules").join("telemaco.mdc").exists());
        // Cursor's hooks.json has its own schema and its prompt hook cannot
        // inject context, so we no longer write one.
        assert!(!path.join(".cursor").join("hooks.json").exists());

        let det = detect_target(TargetId::Cursor, &loc);
        assert!(det.installed);
        assert!(det.already_configured);

        uninstall_target(TargetId::Cursor, &loc, false);
        let mcp = read_json_file(&path.join(".cursor").join("mcp.json"));
        assert!(mcp["mcpServers"]["telemaco"].is_null());
        assert!(!path.join(".cursor").join("rules").join("telemaco.mdc").exists());
        assert!(!still_hooked(&path.join(".cursor").join("hooks.json")));
    }

    #[test]
    fn test_folder_install_and_uninstall_antigravity() {
        let temp = TempDir::new("agy_install");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        let opts = TargetInstallOptions {
            auto_allow: false,
            stealth: true,
            binary_path: "telemaco".to_string(),
            block_builtin_web: true,
            dry_run: false,
        };

        install_target(TargetId::Antigravity, &loc, &opts);
        // Antigravity's workspace customization directory is `.agents`.
        assert!(path.join(".agents").join("mcp_config.json").exists());
        assert!(path.join(".agents").join("hooks.json").exists());
        assert!(path.join(".agents").join("rules").join("telemaco.md").exists());
        // The IDE reads .agents/rules; the CLI reads the project's AGENTS.md.
        assert!(path.join("AGENTS.md").exists(), "the CLI has no directive");

        let hooks = read_json_file(&path.join(".agents").join("hooks.json"));
        assert!(hooks["telemaco"]["PreInvocation"].is_array());

        let det = detect_target(TargetId::Antigravity, &loc);
        assert!(det.installed && det.already_configured);

        uninstall_target(TargetId::Antigravity, &loc, false);
        let mcp = read_json_file(&path.join(".agents").join("mcp_config.json"));
        assert!(mcp["mcpServers"]["telemaco"].is_null());
        let hooks_after = read_json_file(&path.join(".agents").join("hooks.json"));
        assert!(hooks_after["telemaco"].is_null());
    }

    #[test]
    fn test_antigravity_cleans_up_the_paths_it_used_to_write() {
        let temp = TempDir::new("agy_legacy");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());

        // What an earlier Telemaco left in a project.
        let legacy = path.join(".gemini").join("config");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(
            legacy.join("mcp_config.json"),
            "{\"mcpServers\":{\"telemaco\":{\"command\":\"telemaco\"},\"pg\":{\"command\":\"pg\"}}}",
        )
        .unwrap();
        fs::write(
            path.join("AGENTS.md"),
            "# Mine\n\n<!-- TELEMACO_START -->\nold block\n<!-- TELEMACO_END -->\n",
        )
        .unwrap();

        uninstall_target(TargetId::Antigravity, &loc, false);

        let legacy_mcp = read_json_file(&legacy.join("mcp_config.json"));
        assert!(legacy_mcp["mcpServers"]["telemaco"].is_null());
        assert!(legacy_mcp["mcpServers"]["pg"].is_object(), "took a foreign server with it");
        let agents = fs::read_to_string(path.join("AGENTS.md")).unwrap();
        assert!(!agents.contains("TELEMACO_START"), "{}", agents);
        assert!(agents.contains("# Mine"), "{}", agents);
    }

    #[test]
    fn test_folder_install_and_uninstall_codex() {
        let temp = TempDir::new("codex_install");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        let opts = TargetInstallOptions {
            auto_allow: false,
            stealth: true,
            binary_path: "telemaco".to_string(),
            block_builtin_web: true,
            dry_run: false,
        };

        install_target(TargetId::Codex, &loc, &opts);
        assert!(path.join(".codex").join("config.toml").exists());
        assert!(path.join(".codex").join("hooks.json").exists());
        assert!(path.join("AGENTS.md").exists());

        let content = fs::read_to_string(path.join(".codex").join("config.toml")).unwrap();
        // Codex loads hooks by default; the switch stays the user's.
        assert!(!content.contains("[features]"), "{}", content);

        let hooks = read_json_file(&path.join(".codex").join("hooks.json"));
        assert!(hooks["hooks"]["UserPromptSubmit"].is_array());

        let det = detect_target(TargetId::Codex, &loc);
        assert!(det.installed);
        assert!(det.already_configured);

        uninstall_target(TargetId::Codex, &loc, false);
        // config.toml held nothing but our table and the `hooks = true` we
        // added, so it goes away with them.
        let toml_path = path.join(".codex").join("config.toml");
        if toml_path.exists() {
            let content_after = fs::read_to_string(&toml_path).unwrap();
            assert!(!content_after.contains("[mcp_servers.telemaco]"));
            assert!(!content_after.contains("hooks = true"));
        }
        assert!(!still_hooked(&path.join(".codex").join("hooks.json")));
    }

    #[test]
    fn test_folder_install_and_uninstall_gemini() {
        let temp = TempDir::new("gemini_install");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        let opts = TargetInstallOptions {
            auto_allow: false,
            stealth: false,
            binary_path: "telemaco".to_string(),
            block_builtin_web: true,
            dry_run: false,
        };

        install_target(TargetId::Gemini, &loc, &opts);
        assert!(path.join(".gemini").join("settings.json").exists());
        assert!(path.join("GEMINI.md").exists());

        let det = detect_target(TargetId::Gemini, &loc);
        assert!(det.installed);
        assert!(det.already_configured);

        uninstall_target(TargetId::Gemini, &loc, false);
        let settings = read_json_file(&path.join(".gemini").join("settings.json"));
        assert!(settings["mcpServers"]["telemaco"].is_null());
    }

    #[test]
    fn test_folder_install_and_uninstall_windsurf() {
        let temp = TempDir::new("windsurf_install");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        let opts = TargetInstallOptions {
            auto_allow: false,
            stealth: true,
            binary_path: "telemaco".to_string(),
            block_builtin_web: true,
            dry_run: false,
        };

        install_target(TargetId::Windsurf, &loc, &opts);
        // The Devin Local agent is the default one, and it reads the Devin CLI
        // files. `.codeium/windsurf/` has no project form at all.
        let mcp_p = path.join(".devin").join("mcp_config.json");
        assert!(mcp_p.exists());
        assert!(!path.join(".codeium").exists(), "wrote a project path Cascade never opens");
        assert!(path.join(".devin").join("rules").join("telemaco.md").exists());

        let det = detect_target(TargetId::Windsurf, &loc);
        assert!(det.installed);
        assert!(det.already_configured);

        uninstall_target(TargetId::Windsurf, &loc, false);
        let mcp = read_json_file(&mcp_p);
        assert!(mcp["mcpServers"]["telemaco"].is_null());
    }

    #[test]
    fn test_folder_install_and_uninstall_opencode() {
        let temp = TempDir::new("opencode_install");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        let opts = TargetInstallOptions {
            auto_allow: false,
            stealth: true,
            binary_path: "telemaco".to_string(),
            block_builtin_web: true,
            dry_run: false,
        };

        install_target(TargetId::OpenCode, &loc, &opts);
        assert!(path.join("opencode.json").exists());
        assert!(path.join("AGENTS.md").exists());

        let det = detect_target(TargetId::OpenCode, &loc);
        assert!(det.installed);
        assert!(det.already_configured);

        uninstall_target(TargetId::OpenCode, &loc, false);
        let config = read_json_file(&path.join("opencode.json"));
        assert!(config["mcp"]["telemaco"].is_null());
    }

    #[test]
    fn test_folder_install_and_uninstall_roocline() {
        let temp = TempDir::new("roocline_install");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        let opts = TargetInstallOptions {
            auto_allow: false,
            stealth: false,
            binary_path: "telemaco".to_string(),
            block_builtin_web: true,
            dry_run: false,
        };

        install_target(TargetId::RooCline, &loc, &opts);
        assert!(path.join(".roo").join("mcp.json").exists());
        // Two products, two rules paths.
        assert!(path.join(".roo").join("rules").join("telemaco.md").exists());
        assert!(path.join(".clinerules").join("telemaco.md").exists());

        let det = detect_target(TargetId::RooCline, &loc);
        assert!(det.installed);
        assert!(det.already_configured);

        uninstall_target(TargetId::RooCline, &loc, false);
        let mcp = read_json_file(&path.join(".roo").join("mcp.json"));
        assert!(mcp["mcpServers"]["telemaco"].is_null());
        assert!(!path.join(".roo").join("rules").join("telemaco.md").exists());
        assert!(!path.join(".clinerules").join("telemaco.md").exists());
    }

    #[test]
    fn test_roocline_updates_an_existing_clinerules_file() {
        let temp = TempDir::new("roocline_file");
        let path = temp.path();
        // The older single-file form: a directory cannot be created on top of
        // it, so the block goes in the file itself.
        fs::write(path.join(".clinerules"), "# My rules\n").unwrap();

        let loc = Location::Folder(path.to_path_buf());
        let res = install_target(TargetId::RooCline, &loc, &opts_with("telemaco", false, true));
        assert!(res.notes.is_empty(), "unexpected notes: {:?}", res.notes);

        let content = fs::read_to_string(path.join(".clinerules")).unwrap();
        assert!(content.contains("# My rules"), "{}", content);
        assert!(content.contains("<!-- TELEMACO_START -->"), "{}", content);

        uninstall_target(TargetId::RooCline, &loc, false);
        let content = fs::read_to_string(path.join(".clinerules")).unwrap();
        assert!(!content.contains("TELEMACO_START"), "{}", content);
        assert!(content.contains("# My rules"), "{}", content);
    }

    #[test]
    fn test_roocline_global_uses_the_documented_rules_directories() {
        let temp = TempDir::new("roocline_global");
        let home = temp.path().to_path_buf();
        // What an earlier Telemaco wrote where neither product reads.
        fs::write(home.join(".clinerules"), "<!-- TELEMACO_START -->\nold\n<!-- TELEMACO_END -->\n").unwrap();

        roocline::install(&Location::Global, &opts_with("telemaco", false, true), &home);
        assert!(home.join(".roo").join("rules").join("telemaco.md").exists());
        assert!(home.join("Documents").join("Cline").join("Rules").join("telemaco.md").exists());
        // Cline's CLI and SDK read this one; the Documents directory is the
        // VS Code extension's.
        assert!(home.join(".cline").join("rules").join("telemaco.md").exists());

        roocline::uninstall(&Location::Global, &home, false);
        assert!(!home.join(".roo").join("rules").join("telemaco.md").exists());
        assert!(!home.join("Documents").join("Cline").join("Rules").join("telemaco.md").exists());
        assert!(!home.join(".cline").join("rules").join("telemaco.md").exists());
        assert!(!home.join(".clinerules").exists(), "the dead file was left behind");
    }

    #[test]
    fn test_folder_install_and_uninstall_pi() {
        let temp = TempDir::new("pi_install");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        let opts = TargetInstallOptions {
            auto_allow: false,
            stealth: true,
            binary_path: "telemaco".to_string(),
            block_builtin_web: true,
            dry_run: false,
        };

        install_target(TargetId::Pi, &loc, &opts);
        assert!(path.join("AGENTS.md").exists());
        let content = fs::read_to_string(path.join("AGENTS.md")).unwrap();
        assert!(content.contains("<!-- TELEMACO_START -->"));

        let det = detect_target(TargetId::Pi, &loc);
        assert!(det.already_configured);

        uninstall_target(TargetId::Pi, &loc, false);
        if let Ok(content_after) = fs::read_to_string(path.join("AGENTS.md")) {
            assert!(!content_after.contains("<!-- TELEMACO_START -->"));
        }
    }

    #[test]
    fn test_folder_install_and_uninstall_deepseek() {
        let temp = TempDir::new("dsh_install");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        let opts = TargetInstallOptions {
            auto_allow: false,
            stealth: true,
            binary_path: "telemaco".to_string(),
            block_builtin_web: true,
            dry_run: false,
        };

        install_target(TargetId::DeepSeek, &loc, &opts);
        assert!(path.join("AGENTS.md").exists());
        let content = fs::read_to_string(path.join("AGENTS.md")).unwrap();
        assert!(content.contains("<!-- TELEMACO_START -->"));

        let hooks_path = path.join(".dsh").join("hooks.json");
        assert!(hooks_path.exists());
        let hooks_json = read_json_file(&hooks_path);
        let ups = hooks_json["hooks"]["UserPromptSubmit"].as_array().unwrap();
        assert!(ups.iter().any(|g| g["hooks"].as_array().unwrap().iter().any(|h| h["command"] == "telemaco prompt-hook")));

        let det = detect_target(TargetId::DeepSeek, &loc);
        assert!(det.already_configured);

        uninstall_target(TargetId::DeepSeek, &loc, false);
        if let Ok(content_after) = fs::read_to_string(path.join("AGENTS.md")) {
            assert!(!content_after.contains("<!-- TELEMACO_START -->"));
        }
        assert!(!still_hooked(&hooks_path));
    }

    #[test]
    fn test_folder_install_and_uninstall_qwen() {
        let temp = TempDir::new("qwen_install");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        let opts = TargetInstallOptions {
            auto_allow: false,
            stealth: true,
            binary_path: "telemaco".to_string(),
            block_builtin_web: true,
            dry_run: false,
        };

        install_target(TargetId::QwenCode, &loc, &opts);
        assert!(path.join(".qwen").join("settings.json").exists());
        assert!(path.join("AGENTS.md").exists());

        let settings = read_json_file(&path.join(".qwen").join("settings.json"));
        assert!(settings["mcpServers"]["telemaco"].is_object());
        // Qwen parses hook stdout as JSON, so the hook is installed in JSON mode.
        assert!(settings["hooks"]["UserPromptSubmit"].as_array().unwrap().iter().any(|g| g["hooks"].as_array().unwrap().iter().any(|h| h["command"] == "telemaco prompt-hook --format json")));

        let det = detect_target(TargetId::QwenCode, &loc);
        assert!(det.installed);
        assert!(det.already_configured);

        uninstall_target(TargetId::QwenCode, &loc, false);
        let settings_path = path.join(".qwen").join("settings.json");
        assert!(read_json_file(&settings_path)["mcpServers"]["telemaco"].is_null());
        assert!(!still_hooked(&settings_path));
    }

    #[test]
    fn test_folder_install_and_uninstall_droid() {
        let temp = TempDir::new("droid_install");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        let opts = TargetInstallOptions {
            auto_allow: false,
            stealth: true,
            binary_path: "telemaco".to_string(),
            block_builtin_web: true,
            dry_run: false,
        };

        install_target(TargetId::Droid, &loc, &opts);
        assert!(path.join(".factory").join("mcp.json").exists());
        assert!(path.join(".factory").join("hooks.json").exists());
        assert!(path.join("AGENTS.md").exists());

        let mcp = read_json_file(&path.join(".factory").join("mcp.json"));
        assert!(mcp["mcpServers"]["telemaco"].is_object());
        // Keyed directly by event name: Droid ignores a `hooks` wrapper in a
        // standalone hooks.json.
        let hooks = read_json_file(&path.join(".factory").join("hooks.json"));
        assert!(hooks["hooks"].is_null(), "wrapped shape is not read by Droid: {}", hooks);
        assert!(hooks["UserPromptSubmit"].as_array().unwrap().iter().any(|g| g["hooks"].as_array().unwrap().iter().any(|h| h["command"] == "telemaco prompt-hook")));

        let det = detect_target(TargetId::Droid, &loc);
        assert!(det.installed);
        assert!(det.already_configured);

        uninstall_target(TargetId::Droid, &loc, false);
        let mcp_after = read_json_file(&path.join(".factory").join("mcp.json"));
        assert!(mcp_after["mcpServers"]["telemaco"].is_null());
        assert!(!still_hooked(&path.join(".factory").join("hooks.json")));
    }

    #[test]
    fn test_folder_install_and_uninstall_poolside() {
        let temp = TempDir::new("poolside_install");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        let opts = TargetInstallOptions {
            auto_allow: false,
            stealth: true,
            binary_path: "telemaco".to_string(),
            block_builtin_web: true,
            dry_run: false,
        };

        install_target(TargetId::Poolside, &loc, &opts);
        assert!(path.join(".poolside").join("settings.yaml").exists());
        assert!(path.join("AGENTS.md").exists());

        let yaml = fs::read_to_string(path.join(".poolside").join("settings.yaml")).unwrap();
        assert!(yaml.contains("telemaco:"));
        assert!(yaml.contains("telemaco-guard"));

        let det = detect_target(TargetId::Poolside, &loc);
        assert!(det.installed);
        assert!(det.already_configured);

        uninstall_target(TargetId::Poolside, &loc, false);
        // We created settings.yaml, so removing our entries removes the file.
        let yaml_path = path.join(".poolside").join("settings.yaml");
        if yaml_path.exists() {
            let yaml_after = fs::read_to_string(&yaml_path).unwrap();
            assert!(!yaml_after.contains("telemaco:"));
            assert!(!yaml_after.contains("telemaco-guard"));
        }
    }

    /// True when the file still carries a telemaco prompt hook. A file that no
    /// longer exists counts as clean: uninstall deletes configs it created once
    /// nothing is left in them.
    fn still_hooked(path: &Path) -> bool {
        path.exists() && fs::read_to_string(path).map_or(false, |c| c.contains("prompt-hook"))
    }

    fn opts_with(binary: &str, dry_run: bool, block_builtin_web: bool) -> TargetInstallOptions {
        TargetInstallOptions {
            auto_allow: true,
            stealth: true,
            binary_path: binary.to_string(),
            block_builtin_web,
            dry_run,
        }
    }

    #[test]
    fn test_global_install_and_uninstall_uses_home() {
        // Global mode is otherwise untested because it writes into $HOME; the
        // per-target functions take the home directory, so a temp one works.
        let temp = TempDir::new("global_home");
        let home = temp.path().to_path_buf();
        let opts = opts_with("telemaco", false, true);

        let res = claude::install(&Location::Global, &opts, &home);
        assert!(res.notes.is_empty(), "unexpected notes: {:?}", res.notes);
        assert!(home.join(".claude.json").exists());
        assert!(home.join(".claude").join("settings.json").exists());
        assert!(home.join(".claude").join("CLAUDE.md").exists());

        let mcp = read_json_file(&home.join(".claude.json"));
        assert_eq!(mcp["mcpServers"]["telemaco"]["type"], "stdio");

        let det = claude::detect(&Location::Global, Some(&home));
        assert!(det.installed && det.already_configured);

        claude::uninstall(&Location::Global, &home, false);
        let after = read_json_file(&home.join(".claude.json"));
        assert!(after["mcpServers"]["telemaco"].is_null());

        // A second target that keeps its config somewhere else entirely.
        gemini::install(&Location::Global, &opts, &home);
        assert!(home.join(".gemini").join("settings.json").exists());
        assert!(home.join(".gemini").join("GEMINI.md").exists());
        gemini::uninstall(&Location::Global, &home, false);
        let gem = read_json_file(&home.join(".gemini").join("settings.json"));
        assert!(gem["mcpServers"]["telemaco"].is_null());
    }

    #[test]
    fn test_dry_run_writes_nothing() {
        let temp = TempDir::new("dry_run");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());

        let res = install_target(TargetId::Claude, &loc, &opts_with("telemaco", true, true));

        assert!(!res.files.is_empty(), "dry run must still report the plan");
        assert!(res.files.iter().any(|f| f.action == Action::Created));
        for file in &res.files {
            assert!(!file.path.exists(), "dry run wrote {}", file.path.display());
        }
    }

    #[test]
    fn test_web_block_is_optional_and_reversible() {
        let temp = TempDir::new("web_block");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        let settings = path.join(".claude").join("settings.json");

        install_target(TargetId::Claude, &loc, &opts_with("telemaco", false, true));
        let with_block = read_json_file(&settings);
        let groups = with_block["hooks"]["PreToolUse"].as_array().unwrap();
        assert!(groups.iter().any(|g| g["matcher"] == "WebSearch|WebFetch"));

        // Reinstalling with the guard declined has to take it back out.
        install_target(TargetId::Claude, &loc, &opts_with("telemaco", false, false));
        let without = read_json_file(&settings);
        let groups = without["hooks"]["PreToolUse"].as_array().unwrap();
        assert!(!groups.iter().any(|g| g["matcher"] == "WebSearch|WebFetch"));
    }

    #[test]
    fn test_poolside_guard_actually_blocks() {
        let temp = TempDir::new("poolside_guard");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());

        install_target(TargetId::Poolside, &loc, &opts_with("telemaco", false, true));
        let yaml = fs::read_to_string(path.join(".poolside").join("settings.yaml")).unwrap();
        // The old guard ran `telemaco prompt-hook`, which exits 0 and blocks
        // nothing at all. The guard has to be the command that exits 2; the
        // prompt hook belongs on the event that can inject context.
        let guard = yaml
            .lines()
            .skip_while(|l| !l.contains("telemaco-guard"))
            .take(4)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(guard.contains("exit 2"), "{}", yaml);
        assert!(!guard.contains("prompt-hook"), "{}", yaml);
        assert!(yaml.contains("UserPromptSubmit:"), "{}", yaml);
        assert!(yaml.contains("--format poolside"), "{}", yaml);
    }

    #[test]
    fn test_poolside_injects_context_on_its_own_event() {
        let temp = TempDir::new("poolside_context");
        let path = temp.path();
        fs::create_dir_all(path.join(".poolside")).unwrap();
        // A settings file the user already has, with their own hook.
        fs::write(
            path.join(".poolside").join("settings.yaml"),
            "hooks:\n  UserPromptSubmit:\n    - name: house-style\n      matcher: \"*\"\n      command: \"./scripts/style.sh\"\n",
        )
        .unwrap();

        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Poolside, &loc, &opts_with("/opt/bin/telemaco", false, true));

        let yaml = fs::read_to_string(path.join(".poolside").join("settings.yaml")).unwrap();
        assert!(yaml.contains("house-style"), "{}", yaml);
        assert!(yaml.contains("telemaco-context"), "{}", yaml);
        assert_eq!(yaml.matches("UserPromptSubmit:").count(), 1, "{}", yaml);

        // A moved binary is followed, not duplicated.
        install_target(TargetId::Poolside, &loc, &opts_with("/usr/bin/telemaco", false, true));
        let yaml = fs::read_to_string(path.join(".poolside").join("settings.yaml")).unwrap();
        assert_eq!(yaml.matches("telemaco-context").count(), 1, "{}", yaml);
        assert!(yaml.contains("/usr/bin/telemaco prompt-hook --format poolside"), "{}", yaml);

        uninstall_target(TargetId::Poolside, &loc, false);
        let yaml = fs::read_to_string(path.join(".poolside").join("settings.yaml")).unwrap();
        assert!(!yaml.contains("telemaco"), "{}", yaml);
        assert!(yaml.contains("house-style"), "{}", yaml);
    }

    #[test]
    fn test_poolside_does_not_duplicate_a_top_level_key() {
        let temp = TempDir::new("poolside_yaml");
        let path = temp.path();
        fs::create_dir_all(path.join(".poolside")).unwrap();
        let settings = path.join(".poolside").join("settings.yaml");
        fs::write(&settings, "hooks:\n  PreToolUse:\n    - name: other-guard\n      command: \"true\"\n").unwrap();

        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Poolside, &loc, &opts_with("telemaco", false, true));

        let yaml = fs::read_to_string(&settings).unwrap();
        let hook_keys = yaml.lines().filter(|l| *l == "hooks:").count();
        assert_eq!(hook_keys, 1, "duplicate top-level key makes the YAML invalid:\n{}", yaml);
        // The nested key must not be duplicated either: the guard belongs in
        // the PreToolUse list that is already there.
        let pretooluse_keys = yaml.lines().filter(|l| *l == "  PreToolUse:").count();
        assert_eq!(pretooluse_keys, 1, "duplicate nested key:\n{}", yaml);
        assert!(yaml.contains("other-guard"), "existing hook was dropped:\n{}", yaml);
        assert!(yaml.contains("telemaco-guard"));
        assert!(yaml.contains("mcp_servers:"));
    }

    #[test]
    fn test_config_snippet_matches_what_is_installed() {
        let temp = TempDir::new("snippet");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());

        install_target(TargetId::Claude, &loc, &opts_with("/opt/bin/telemaco", false, true));
        let installed = read_json_file(&path.join(".mcp.json"));

        let snippet = config_snippet(TargetId::Claude, "/opt/bin/telemaco", true, true);
        let parsed: serde_json::Value = serde_json::from_str(&snippet).unwrap();
        assert_eq!(parsed["mcpServers"]["telemaco"], installed["mcpServers"]["telemaco"]);
    }

    #[test]
    fn test_existing_config_is_backed_up_before_rewrite() {
        let temp = TempDir::new("backup");
        let path = temp.path();
        let mcp_path = path.join(".mcp.json");
        let original = "{\n  \"mcpServers\": {\n    \"pg\": {\n      \"command\": \"pg-mcp\"\n    }\n  }\n}\n";
        fs::write(&mcp_path, original).unwrap();

        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Claude, &loc, &opts_with("telemaco", false, true));

        let backup = path.join(".mcp.json.telemaco-backup");
        assert!(backup.exists());
        assert_eq!(fs::read_to_string(&backup).unwrap(), original);
    }

    #[test]
    fn test_unpaired_markers_leave_the_file_alone() {
        let temp = TempDir::new("markers");
        let path = temp.path();
        // A start marker with no end: a truncated block, or one an editor cut.
        let original = "# My rules\n\n<!-- TELEMACO_START -->\ntruncated\n\nTEXT THE USER CARES ABOUT\n";
        fs::write(path.join("CLAUDE.md"), original).unwrap();

        let loc = Location::Folder(path.to_path_buf());
        let res = install_target(TargetId::Claude, &loc, &opts_with("telemaco", false, true));

        // Appending a second block used to make the stray start pair up with
        // the new end, and the next run replaced the user's text in between.
        assert_eq!(fs::read_to_string(path.join("CLAUDE.md")).unwrap(), original);
        assert!(
            res.notes.iter().any(|n| n.contains("markers")),
            "expected a note, got {:?}",
            res.notes
        );

        // Two complete blocks are just as ambiguous.
        let two = "<!-- TELEMACO_START -->\nA\n<!-- TELEMACO_END -->\n\nuser text\n\n<!-- TELEMACO_START -->\nB\n<!-- TELEMACO_END -->\n";
        fs::write(path.join("CLAUDE.md"), two).unwrap();
        let res = install_target(TargetId::Claude, &loc, &opts_with("telemaco", false, true));
        assert_eq!(fs::read_to_string(path.join("CLAUDE.md")).unwrap(), two);
        assert!(res.notes.iter().any(|n| n.contains("markers")));

        // One well-formed block still updates normally.
        fs::write(path.join("CLAUDE.md"), "# My rules\n").unwrap();
        install_target(TargetId::Claude, &loc, &opts_with("telemaco", false, true));
        let content = fs::read_to_string(path.join("CLAUDE.md")).unwrap();
        assert_eq!(content.matches("<!-- TELEMACO_START -->").count(), 1);
        assert!(content.contains("# My rules"));
    }

    #[test]
    fn test_no_permissions_revokes_an_earlier_grant() {
        let temp = TempDir::new("revoke");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());

        install_target(TargetId::Claude, &loc, &opts_with("telemaco", false, true));
        install_target(TargetId::Kiro, &loc, &opts_with("telemaco", false, true));

        let settings = path.join(".claude").join("settings.json");
        let kiro_mcp = path.join(".kiro").join("settings").join("mcp.json");
        // Claude Code auto-approves through `permissions.allow`; there is no
        // `autoApprove` setting.
        assert_eq!(
            read_json_file(&settings)["permissions"]["allow"],
            serde_json::json!(["mcp__telemaco__*"])
        );
        assert_eq!(
            read_json_file(&kiro_mcp)["mcpServers"]["telemaco"]["autoApprove"],
            serde_json::json!(["*"])
        );

        // Reinstalling without permissions has to take the grant back.
        let no_perms = TargetInstallOptions {
            auto_allow: false,
            stealth: true,
            binary_path: "telemaco".to_string(),
            block_builtin_web: true,
            dry_run: false,
        };
        install_target(TargetId::Claude, &loc, &no_perms);
        install_target(TargetId::Kiro, &loc, &no_perms);

        assert_eq!(
            read_json_file(&settings)["permissions"]["allow"],
            serde_json::json!([])
        );
        assert_eq!(
            read_json_file(&kiro_mcp)["mcpServers"]["telemaco"]["autoApprove"],
            serde_json::json!([])
        );
    }

    #[test]
    fn test_roocline_is_not_detected_from_bare_vscode_storage() {
        let temp = TempDir::new("roo_global");
        let home = temp.path().to_path_buf();
        // A VS Code install with neither extension present.
        #[cfg(target_os = "macos")]
        let storage = home.join("Library/Application Support/Code/User/globalStorage");
        #[cfg(not(target_os = "macos"))]
        let storage = home.join(".config/Code/User/globalStorage");
        fs::create_dir_all(storage.join("some.other-extension")).unwrap();

        let det = roocline::detect(&Location::Global, Some(&home));
        assert!(!det.installed, "hint was {:?}", det.hint);

        // With the extension's settings file there, it is a real detection.
        let roo = storage
            .join("rooveterinaryinc.roo-cline")
            .join("settings")
            .join("mcp_settings.json");
        fs::create_dir_all(roo.parent().unwrap()).unwrap();
        fs::write(&roo, "{}").unwrap();
        assert!(roocline::detect(&Location::Global, Some(&home)).installed);
    }

    #[test]
    fn test_poolside_matches_the_indentation_already_in_the_file() {
        let temp = TempDir::new("yaml_indent");
        let path = temp.path();
        fs::create_dir_all(path.join(".poolside")).unwrap();
        let settings = path.join(".poolside").join("settings.yaml");
        // Four-space indentation is just as valid as two.
        fs::write(
            &settings,
            "mcp_servers:\n    other:\n        command: \"x\"\nhooks:\n    PreToolUse:\n        - name: other-guard\n          command: \"true\"\n",
        )
        .unwrap();

        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Poolside, &loc, &opts_with("telemaco", false, true));

        let yaml = fs::read_to_string(&settings).unwrap();
        assert_eq!(yaml.lines().filter(|l| l.trim() == "PreToolUse:").count(), 1, "{}", yaml);
        assert_eq!(yaml.lines().filter(|l| *l == "mcp_servers:").count(), 1, "{}", yaml);
        // Our entry has to be a sibling of theirs, not a child of it.
        assert!(yaml.contains("\n    telemaco:\n"), "wrong indent:\n{}", yaml);
        assert!(yaml.contains("\n        - name: telemaco-guard\n"), "wrong indent:\n{}", yaml);
        assert!(yaml.contains("other-guard"));
        assert!(yaml.contains("\n    other:\n"), "the user's server moved:\n{}", yaml);
    }

    #[test]
    fn test_mcp_entry_keeps_keys_the_user_added() {
        let temp = TempDir::new("entry_merge");
        let path = temp.path();
        fs::write(
            path.join(".mcp.json"),
            r#"{"mcpServers":{"telemaco":{"command":"telemaco","args":["mcp"],"env":{"HTTP_PROXY":"http://corp:8080"},"timeout":60000}}}"#,
        )
        .unwrap();

        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Claude, &loc, &opts_with("/opt/bin/telemaco", false, true));

        let entry = &read_json_file(&path.join(".mcp.json"))["mcpServers"]["telemaco"];
        // Ours win.
        assert_eq!(entry["command"], "/opt/bin/telemaco");
        // Theirs survive.
        assert_eq!(entry["env"]["HTTP_PROXY"], "http://corp:8080");
        assert_eq!(entry["timeout"], 60000);
    }

    #[test]
    fn test_roocline_uses_the_documented_project_path() {
        let temp = TempDir::new("roo_path");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());

        install_target(TargetId::RooCline, &loc, &opts_with("telemaco", false, true));

        // Roo Code reads .roo/mcp.json; .vscode/mcp.json is VS Code's own file
        // and is read by neither Roo Code nor Cline.
        let roo = path.join(".roo").join("mcp.json");
        assert!(roo.exists());
        assert!(read_json_file(&roo)["mcpServers"]["telemaco"].is_object());
        assert!(!path.join(".vscode").join("mcp.json").exists());
    }

    #[test]
    fn test_roocline_uninstall_cleans_the_legacy_path() {
        let temp = TempDir::new("roo_legacy");
        let path = temp.path();
        fs::create_dir_all(path.join(".vscode")).unwrap();
        fs::write(
            path.join(".vscode").join("mcp.json"),
            r#"{"mcpServers":{"telemaco":{"command":"telemaco"},"pg":{"command":"pg-mcp"}}}"#,
        )
        .unwrap();

        let loc = Location::Folder(path.to_path_buf());
        uninstall_target(TargetId::RooCline, &loc, false);

        let legacy = read_json_file(&path.join(".vscode").join("mcp.json"));
        assert!(legacy["mcpServers"]["telemaco"].is_null());
        assert_eq!(legacy["mcpServers"]["pg"]["command"], "pg-mcp");
    }

    #[test]
    fn test_roocline_global_writes_the_names_each_product_reads() {
        let temp = TempDir::new("roo_global_mcp");
        let home = temp.path().to_path_buf();
        #[cfg(target_os = "macos")]
        let storage = home.join("Library/Application Support/Code/User/globalStorage");
        #[cfg(not(target_os = "macos"))]
        let storage = home.join(".config/Code/User/globalStorage");
        // Both extensions installed, plus the Cline CLI.
        let roo_base = storage.join("rooveterinaryinc.roo-cline");
        let cline_ext = storage.join("saoudrizwan.claude-dev");
        fs::create_dir_all(roo_base.join("settings")).unwrap();
        fs::create_dir_all(cline_ext.join("settings")).unwrap();
        fs::create_dir_all(home.join(".cline").join("data").join("settings")).unwrap();

        roocline::install(&Location::Global, &opts_with("telemaco", false, true), &home);

        // Roo reads mcp_settings.json: cline_mcp_settings.json is the name it
        // inherited from the fork and stopped reading.
        let roo_mcp = roo_base.join("settings").join("mcp_settings.json");
        assert!(roo_mcp.exists(), "Roo's global server went to a file it does not read");
        assert!(read_json_file(&roo_mcp)["mcpServers"]["telemaco"].is_object());
        assert!(!roo_base.join("settings").join("cline_mcp_settings.json").exists());

        // Cline's extension keeps the fork's name.
        let ext_mcp = cline_ext.join("settings").join("cline_mcp_settings.json");
        assert!(read_json_file(&ext_mcp)["mcpServers"]["telemaco"].is_object());

        // Cline's CLI and SDK read the file under its own home, not the
        // ~/.cline/mcp.json the MCP page still documents.
        let cli_mcp = home
            .join(".cline")
            .join("data")
            .join("settings")
            .join("cline_mcp_settings.json");
        assert!(read_json_file(&cli_mcp)["mcpServers"]["telemaco"].is_object());
        assert!(!home.join(".cline").join("mcp.json").exists());

        roocline::uninstall(&Location::Global, &home, false);
        assert!(read_json_file(&roo_mcp)["mcpServers"]["telemaco"].is_null());
        assert!(read_json_file(&ext_mcp)["mcpServers"]["telemaco"].is_null());
        assert!(read_json_file(&cli_mcp)["mcpServers"]["telemaco"].is_null());
    }

    #[test]
    fn test_roocline_follows_roos_custom_storage_path() {
        let temp = TempDir::new("roo_custom_storage");
        let home = temp.path().to_path_buf();
        let elsewhere = home.join("roo-elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        #[cfg(target_os = "macos")]
        let user_dir = home.join("Library/Application Support/Code/User");
        #[cfg(not(target_os = "macos"))]
        let user_dir = home.join(".config/Code/User");
        fs::create_dir_all(&user_dir).unwrap();
        // VS Code settings carry comments; the reader has to cope.
        fs::write(
            user_dir.join("settings.json"),
            format!(
                "{{\n  // moved off the system disk\n  \"roo-cline.customStoragePath\": {:?},\n}}\n",
                elsewhere.to_string_lossy()
            ),
        )
        .unwrap();

        roocline::install(&Location::Global, &opts_with("telemaco", false, true), &home);

        let moved = elsewhere.join("settings").join("mcp_settings.json");
        assert!(moved.exists(), "the custom storage path was ignored");
        assert!(read_json_file(&moved)["mcpServers"]["telemaco"].is_object());
        assert!(!user_dir
            .join("globalStorage")
            .join("rooveterinaryinc.roo-cline")
            .join("settings")
            .join("mcp_settings.json")
            .exists());

        assert!(roocline::detect(&Location::Global, Some(&home)).installed);
        roocline::uninstall(&Location::Global, &home, false);
        assert!(read_json_file(&moved)["mcpServers"]["telemaco"].is_null());
    }

    #[test]
    fn test_roocline_global_uninstall_cleans_the_paths_the_docs_still_name() {
        let temp = TempDir::new("roo_global_legacy");
        let home = temp.path().to_path_buf();
        #[cfg(target_os = "macos")]
        let storage = home.join("Library/Application Support/Code/User/globalStorage");
        #[cfg(not(target_os = "macos"))]
        let storage = home.join(".config/Code/User/globalStorage");
        let roo_legacy = storage
            .join("rooveterinaryinc.roo-cline")
            .join("settings")
            .join("cline_mcp_settings.json");
        fs::create_dir_all(roo_legacy.parent().unwrap()).unwrap();
        fs::write(&roo_legacy, r#"{"mcpServers":{"telemaco":{"command":"telemaco"},"pg":{"command":"pg-mcp"}}}"#).unwrap();
        let cline_legacy = home.join(".cline").join("mcp.json");
        fs::create_dir_all(cline_legacy.parent().unwrap()).unwrap();
        fs::write(&cline_legacy, r#"{"mcpServers":{"telemaco":{"command":"telemaco"}}}"#).unwrap();

        roocline::uninstall(&Location::Global, &home, false);

        let left = read_json_file(&roo_legacy);
        assert!(left["mcpServers"]["telemaco"].is_null());
        assert_eq!(left["mcpServers"]["pg"]["command"], "pg-mcp");
        // Ours was the only entry, so the dead file goes with it.
        assert!(!cline_legacy.exists(), "the unread file was left behind");
    }

    #[test]
    fn test_roocline_detects_a_cline_cli_that_has_no_mcp_file_yet() {
        let temp = TempDir::new("roo_cli_only");
        let home = temp.path().to_path_buf();
        // What `cline` leaves behind after a first run: settings, no servers.
        fs::create_dir_all(home.join(".cline").join("data").join("settings")).unwrap();

        let det = roocline::detect(&Location::Global, Some(&home));
        assert!(det.installed, "hint was {:?}", det.hint);
        assert!(!det.already_configured);
    }

    #[test]
    fn test_gemini_writes_the_context_file_the_settings_name() {
        let temp = TempDir::new("gemini_context_name");
        let path = temp.path();
        fs::create_dir_all(path.join(".gemini")).unwrap();
        // `context.fileName` replaces GEMINI.md; a list means every name is
        // loaded, so the first one is the one to write.
        fs::write(
            path.join(".gemini").join("settings.json"),
            r#"{"context":{"fileName":["AGENTS.md","GEMINI.md"]},"ui":{"theme":"GitHub"}}"#,
        )
        .unwrap();

        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Gemini, &loc, &opts_with("telemaco", false, true));

        let agents = fs::read_to_string(path.join("AGENTS.md")).unwrap();
        assert!(agents.contains("<!-- TELEMACO_START -->"), "{}", agents);
        assert!(!path.join("GEMINI.md").exists(), "wrote the name Gemini was told to ignore");

        // The user's own settings survive the MCP and hook write.
        let settings = read_json_file(&path.join(".gemini").join("settings.json"));
        assert_eq!(settings["ui"]["theme"], "GitHub");
        assert_eq!(settings["context"]["fileName"][0], "AGENTS.md");

        uninstall_target(TargetId::Gemini, &loc, false);
        assert!(!path.join("AGENTS.md").exists());
    }

    #[test]
    fn test_gemini_uninstall_finds_a_block_left_under_the_old_name() {
        let temp = TempDir::new("gemini_context_switch");
        let path = temp.path();
        // Installed while the default was in force, then the user renamed the
        // context file. The block is still in GEMINI.md and has to go.
        fs::write(
            path.join("GEMINI.md"),
            "<!-- TELEMACO_START -->\nold directive\n<!-- TELEMACO_END -->\n",
        )
        .unwrap();
        fs::create_dir_all(path.join(".gemini")).unwrap();
        fs::write(
            path.join(".gemini").join("settings.json"),
            r#"{"contextFileName":"CONTEXT.md"}"#,
        )
        .unwrap();

        let loc = Location::Folder(path.to_path_buf());
        uninstall_target(TargetId::Gemini, &loc, false);

        assert!(!path.join("GEMINI.md").exists(), "the stranded block was left behind");
    }

    #[test]
    fn test_gemini_does_not_claim_a_hook_the_user_wrote() {
        let temp = TempDir::new("gemini_foreign_hook");
        let path = temp.path();
        fs::create_dir_all(path.join(".gemini")).unwrap();
        fs::write(
            path.join(".gemini").join("settings.json"),
            r#"{"hooks":{"BeforeAgent":[{"hooks":[{"type":"command","command":"./scripts/lint.sh"}]}]}}"#,
        )
        .unwrap();

        let loc = Location::Folder(path.to_path_buf());
        let det = detect_target(TargetId::Gemini, &loc);
        assert!(det.installed);
        assert!(!det.already_configured, "someone else's hook counted as ours");

        install_target(TargetId::Gemini, &loc, &opts_with("telemaco", false, true));
        assert!(detect_target(TargetId::Gemini, &loc).already_configured);

        // Ours goes in beside theirs, not on top of it.
        let settings = read_json_file(&path.join(".gemini").join("settings.json"));
        let groups = settings["hooks"]["BeforeAgent"].as_array().unwrap();
        assert_eq!(groups.len(), 2, "{:#?}", groups);
        assert_eq!(groups[0]["hooks"][0]["command"], "./scripts/lint.sh");

        uninstall_target(TargetId::Gemini, &loc, false);
        let settings = read_json_file(&path.join(".gemini").join("settings.json"));
        assert_eq!(settings["hooks"]["BeforeAgent"][0]["hooks"][0]["command"], "./scripts/lint.sh");
    }

    #[test]
    fn test_qwen_writes_the_context_file_the_settings_name() {
        let temp = TempDir::new("qwen_context_name");
        let path = temp.path();
        fs::create_dir_all(path.join(".qwen")).unwrap();
        fs::write(
            path.join(".qwen").join("settings.json"),
            r#"{"context":{"fileName":"CONTEXT.md"}}"#,
        )
        .unwrap();

        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::QwenCode, &loc, &opts_with("telemaco", false, true));

        assert!(path.join("CONTEXT.md").exists(), "wrote a name Qwen no longer loads");
        assert!(!path.join("AGENTS.md").exists());
        assert!(!path.join("QWEN.md").exists());

        uninstall_target(TargetId::QwenCode, &loc, false);
        assert!(!path.join("CONTEXT.md").exists());
    }

    #[test]
    fn test_qwen_keeps_sharing_agents_md_when_nothing_is_configured() {
        let temp = TempDir::new("qwen_default_name");
        let path = temp.path();
        fs::create_dir_all(path.join(".qwen")).unwrap();
        fs::write(path.join(".qwen").join("settings.json"), r#"{"ui":{"theme":"Dracula"}}"#).unwrap();

        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::QwenCode, &loc, &opts_with("telemaco", false, true));

        // Qwen reads AGENTS.md too, so the block goes where the other agents
        // already look instead of adding a second file.
        assert!(path.join("AGENTS.md").exists());
        assert!(!path.join("QWEN.md").exists());

        uninstall_target(TargetId::QwenCode, &loc, false);
        assert!(!path.join("AGENTS.md").exists());
    }

    #[test]
    fn test_qwen_does_not_claim_a_hook_the_user_wrote() {
        let temp = TempDir::new("qwen_foreign_hook");
        let path = temp.path();
        fs::create_dir_all(path.join(".qwen")).unwrap();
        fs::write(
            path.join(".qwen").join("settings.json"),
            r#"{"hooks":{"UserPromptSubmit":[{"hooks":[{"type":"command","command":"./scripts/audit.sh"}]}]}}"#,
        )
        .unwrap();

        let loc = Location::Folder(path.to_path_buf());
        assert!(!detect_target(TargetId::QwenCode, &loc).already_configured);

        install_target(TargetId::QwenCode, &loc, &opts_with("telemaco", false, true));
        assert!(detect_target(TargetId::QwenCode, &loc).already_configured);

        uninstall_target(TargetId::QwenCode, &loc, false);
        let settings = read_json_file(&path.join(".qwen").join("settings.json"));
        assert_eq!(
            settings["hooks"]["UserPromptSubmit"][0]["hooks"][0]["command"],
            "./scripts/audit.sh"
        );
    }

    #[test]
    fn test_pi_writes_the_override_file_when_the_project_has_one() {
        let temp = TempDir::new("pi_override");
        let path = temp.path();
        fs::create_dir_all(path.join(".pi")).unwrap();
        // Pi loads AGENTS.override.md instead of AGENTS.md from a directory
        // that has one, exactly as Codex does.
        fs::write(path.join("AGENTS.override.md"), "# Mine\n").unwrap();
        fs::write(path.join("AGENTS.md"), "# Shared\n").unwrap();

        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Pi, &loc, &opts_with("telemaco", false, true));

        let override_md = fs::read_to_string(path.join("AGENTS.override.md")).unwrap();
        assert!(override_md.contains("<!-- TELEMACO_START -->"), "{}", override_md);
        let shared = fs::read_to_string(path.join("AGENTS.md")).unwrap();
        assert!(!shared.contains("TELEMACO_START"), "wrote the file Pi ignores:\n{}", shared);

        uninstall_target(TargetId::Pi, &loc, false);
        let override_md = fs::read_to_string(path.join("AGENTS.override.md")).unwrap();
        assert!(!override_md.contains("TELEMACO_START"));
        assert!(override_md.contains("# Mine"));
    }

    #[test]
    fn test_deepseek_global_registers_the_mcp_plugin_in_the_patch_layer() {
        let temp = TempDir::new("dsh_patch");
        let home = temp.path().to_path_buf();
        fs::create_dir_all(home.join(".dsh")).unwrap();
        // A patch layer the user already has: it must survive.
        fs::write(
            home.join(".dsh").join("cordis.patch.yml"),
            "# my patches\n- insert:\n    - id: memory-mcp-reference\n      name: '@deepseek-ai/dsh-mcp-client'\n      config:\n        serverName: reference_memory\n",
        )
        .unwrap();

        deepseek::install(&Location::Global, &opts_with("/opt/bin/telemaco", false, true), &home);

        let patch = fs::read_to_string(home.join(".dsh").join("cordis.patch.yml")).unwrap();
        assert!(patch.contains("id: memory-mcp-reference"), "{}", patch);
        assert!(patch.contains("id: telemaco-mcp"), "{}", patch);
        assert!(patch.contains("serverName: telemaco"), "{}", patch);
        assert!(patch.contains("command: \"/opt/bin/telemaco\""), "{}", patch);
        assert!(patch.contains("- \"--stealth\""), "{}", patch);

        // Reinstalling from a new location rewrites our command, not theirs.
        deepseek::install(&Location::Global, &opts_with("/usr/local/bin/telemaco", false, true), &home);
        let patch = fs::read_to_string(home.join(".dsh").join("cordis.patch.yml")).unwrap();
        assert_eq!(patch.matches("id: telemaco-mcp").count(), 1, "{}", patch);
        assert!(patch.contains("command: \"/usr/local/bin/telemaco\""), "{}", patch);
        assert!(!patch.contains("/opt/bin/telemaco"), "the old path stayed:\n{}", patch);
        assert!(patch.contains("serverName: reference_memory"), "{}", patch);

        deepseek::uninstall(&Location::Global, &home, false);
        let patch = fs::read_to_string(home.join(".dsh").join("cordis.patch.yml")).unwrap();
        assert!(!patch.contains("telemaco-mcp"), "{}", patch);
        assert!(patch.contains("id: memory-mcp-reference"), "{}", patch);
        assert!(patch.contains("# my patches"), "the user's comment went with it:\n{}", patch);
    }

    #[test]
    fn test_deepseek_patch_layer_round_trips_to_nothing() {
        let temp = TempDir::new("dsh_patch_solo");
        let home = temp.path().to_path_buf();
        fs::create_dir_all(home.join(".dsh")).unwrap();

        deepseek::install(&Location::Global, &opts_with("telemaco", false, true), &home);
        assert!(home.join(".dsh").join("cordis.patch.yml").exists());

        deepseek::uninstall(&Location::Global, &home, false);
        assert!(
            !home.join(".dsh").join("cordis.patch.yml").exists(),
            "ours was the only patch, so the file goes with it"
        );
    }

    #[test]
    fn test_kiro_is_not_detected_from_an_agents_md_that_names_it() {
        let temp = TempDir::new("kiro_false_positive");
        let path = temp.path();
        fs::write(
            path.join("AGENTS.md"),
            "# Contributing\n\nThe installer supports kiro among others.\n",
        )
        .unwrap();

        let det = detect_target(TargetId::Kiro, &Location::Folder(path.to_path_buf()));
        assert!(!det.installed, "hint was {:?}", det.hint);

        // The directory Kiro owns still identifies it.
        fs::create_dir_all(path.join(".kiro")).unwrap();
        assert!(detect_target(TargetId::Kiro, &Location::Folder(path.to_path_buf())).installed);
    }

    #[test]
    fn test_kiro_hook_uses_the_pascal_case_trigger() {
        let temp = TempDir::new("kiro_trigger");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());

        install_target(TargetId::Kiro, &loc, &opts_with("telemaco", false, true));

        let hooks = read_json_file(&path.join(".kiro").join("hooks").join("telemaco.json"));
        assert_eq!(hooks["version"], "v1");
        // The v1 schema names triggers in PascalCase; `promptSubmit` is the
        // 0.x name that maps onto this one.
        assert_eq!(hooks["hooks"][0]["trigger"], "UserPromptSubmit");
        assert_eq!(hooks["hooks"][0]["action"]["type"], "command");

        let det = detect_target(TargetId::Kiro, &loc);
        assert!(det.installed);
        assert!(det.already_configured);

        uninstall_target(TargetId::Kiro, &loc, false);
        assert!(!path.join(".kiro").join("hooks").join("telemaco.json").exists());
    }

    #[test]
    fn test_poolside_handles_inline_keys() {
        let temp = TempDir::new("yaml_inline");
        let path = temp.path();
        fs::create_dir_all(path.join(".poolside")).unwrap();
        let settings = path.join(".poolside").join("settings.yaml");
        // An empty inline mapping can be turned into a block.
        fs::write(&settings, "hooks: {}\nmodel: gpt\n").unwrap();

        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Poolside, &loc, &opts_with("telemaco", false, true));

        let yaml = fs::read_to_string(&settings).unwrap();
        let hook_keys = yaml.lines().filter(|l| l.starts_with("hooks:")).count();
        assert_eq!(hook_keys, 1, "duplicate top-level key:\n{}", yaml);
        assert!(yaml.contains("telemaco-guard"), "{}", yaml);

        // A populated inline mapping cannot be merged into safely, so it is
        // reported and left as it is rather than duplicated.
        let temp2 = TempDir::new("yaml_inline2");
        let path2 = temp2.path();
        fs::create_dir_all(path2.join(".poolside")).unwrap();
        let settings2 = path2.join(".poolside").join("settings.yaml");
        fs::write(&settings2, "hooks: {PreToolUse: []}\n").unwrap();
        let loc2 = Location::Folder(path2.to_path_buf());
        let res = install_target(TargetId::Poolside, &loc2, &opts_with("telemaco", false, true));

        let yaml2 = fs::read_to_string(&settings2).unwrap();
        assert_eq!(yaml2.lines().filter(|l| l.starts_with("hooks:")).count(), 1, "{}", yaml2);
        assert!(
            res.notes.iter().any(|n| n.contains("inline value")),
            "expected a note, got {:?}",
            res.notes
        );
    }

    #[test]
    fn test_codex_leaves_the_hooks_switch_alone() {
        // Codex runs hooks by default and `[features] hooks = false` is how a
        // user turns them off. Writing `true` over that took a decision away
        // from them; the install says what it found instead.
        let temp = TempDir::new("codex_flag");
        let path = temp.path();
        fs::create_dir_all(path.join(".codex")).unwrap();
        let toml_path = path.join(".codex").join("config.toml");
        fs::write(
            &toml_path,
            "[features]\nhooks = false\n\n[profiles.work]\nhooks = false\nmodel = \"x\"\n",
        )
        .unwrap();

        let loc = Location::Folder(path.to_path_buf());
        let res = install_target(TargetId::Codex, &loc, &opts_with("telemaco", false, true));

        let content = fs::read_to_string(&toml_path).unwrap();
        assert!(content.contains("[features]\nhooks = false"), "{}", content);
        assert!(content.contains("[profiles.work]\nhooks = false"), "{}", content);
        assert!(
            res.notes.iter().any(|n| n.contains("hooks = false")),
            "expected a note about the disabled hooks, got {:?}",
            res.notes
        );
    }

    #[test]
    fn test_codex_install_adds_no_feature_flag() {
        let temp = TempDir::new("codex_noflag");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Codex, &loc, &opts_with("telemaco", false, true));
        let content = fs::read_to_string(path.join(".codex").join("config.toml")).unwrap();
        assert!(!content.contains("[features]"), "{}", content);
    }

    #[test]
    fn test_codex_migrates_the_old_top_level_flag() {
        let temp = TempDir::new("codex_migrate");
        let path = temp.path();
        fs::create_dir_all(path.join(".codex")).unwrap();
        let toml_path = path.join(".codex").join("config.toml");
        // What an earlier Telemaco left behind.
        fs::write(&toml_path, "hooks = true\nmodel = \"x\"\n").unwrap();

        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Codex, &loc, &opts_with("telemaco", false, true));
        let content = fs::read_to_string(&toml_path).unwrap();
        assert!(!content.contains("\nhooks = true\nmodel"), "{}", content);
        // Nothing takes its place: hooks are on unless the user says otherwise.
        assert!(!content.contains("[features]"), "{}", content);
        assert!(content.contains("model = \"x\""), "{}", content);

        uninstall_target(TargetId::Codex, &loc, false);
        let after = fs::read_to_string(&toml_path).unwrap();
        assert!(!after.contains("hooks = true"), "{}", after);
        assert!(!after.contains("[features]"), "{}", after);
        assert!(after.contains("model = \"x\""), "{}", after);
    }

    #[test]
    fn test_codex_uninstall_cleans_the_flag_an_older_version_wrote() {
        let temp = TempDir::new("codex_oldflag");
        let path = temp.path();
        fs::create_dir_all(path.join(".codex")).unwrap();
        let toml_path = path.join(".codex").join("config.toml");
        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Codex, &loc, &opts_with("telemaco", false, true));

        // What an older Telemaco left in the file next to our table.
        let with_flag = format!(
            "[features]\nhooks = true\n\n{}",
            fs::read_to_string(&toml_path).unwrap()
        );
        fs::write(&toml_path, with_flag).unwrap();

        uninstall_target(TargetId::Codex, &loc, false);
        let after = fs::read_to_string(&toml_path).unwrap_or_default();
        assert!(!after.contains("[features]"), "{}", after);
    }

    #[test]
    fn test_codex_uninstall_keeps_a_hooks_switch_the_user_set() {
        let temp = TempDir::new("codex_userflag");
        let path = temp.path();
        fs::create_dir_all(path.join(".codex")).unwrap();
        let toml_path = path.join(".codex").join("config.toml");
        fs::write(&toml_path, "[features]\nhooks = false\n").unwrap();

        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Codex, &loc, &opts_with("telemaco", false, true));
        uninstall_target(TargetId::Codex, &loc, false);

        let after = fs::read_to_string(&toml_path).unwrap();
        assert!(after.contains("hooks = false"), "took the user's switch out: {}", after);
    }

    #[test]
    fn test_codex_keeps_user_keys_in_our_table() {
        let temp = TempDir::new("codex_keys");
        let path = temp.path();
        fs::create_dir_all(path.join(".codex")).unwrap();
        let toml_path = path.join(".codex").join("config.toml");
        fs::write(
            &toml_path,
            "[mcp_servers.telemaco]\ncommand = \"telemaco\"\nargs = [\"mcp\"]\nstartup_timeout_ms = 30000\n",
        )
        .unwrap();

        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Codex, &loc, &opts_with("/opt/bin/telemaco", false, true));

        let content = fs::read_to_string(&toml_path).unwrap();
        assert!(content.contains("command = \"/opt/bin/telemaco\""), "{}", content);
        assert!(content.contains("startup_timeout_ms = 30000"), "user key dropped:\n{}", content);
    }

    #[test]
    fn test_uninstall_leaves_no_empty_husks() {
        let temp = TempDir::new("husks");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        let opts = opts_with("telemaco", false, true);

        for &id in TargetId::all() {
            install_target(id, &loc, &opts);
        }
        for &id in TargetId::all() {
            uninstall_target(id, &loc, false);
        }

        let mut leftovers = Vec::new();
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            let Ok(entries) = fs::read_dir(dir) else { return };
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if !p.to_string_lossy().ends_with(".telemaco-backup") {
                    out.push(p);
                }
            }
        }
        walk(path, &mut leftovers);

        // The directory started empty, so every file here was created by the
        // installer and must be gone again.
        assert!(
            leftovers.is_empty(),
            "uninstall left {:?}",
            leftovers
                .iter()
                .map(|f| format!("{} = {}", f.display(), fs::read_to_string(f).unwrap_or_default().trim()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_reinstall_does_not_pile_up_backups() {
        let temp = TempDir::new("backup_pile");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        let opts = opts_with("telemaco", false, true);

        // First install creates the files, so there is no earlier state to keep.
        install_target(TargetId::Claude, &loc, &opts);
        install_target(TargetId::Claude, &loc, &opts_with("/opt/bin/telemaco", false, true));

        assert!(!path.join(".mcp.json.telemaco-backup").exists());
        assert!(!path.join("CLAUDE.md.telemaco-backup").exists());

        // A file that existed before us is still backed up.
        let temp2 = TempDir::new("backup_real");
        let path2 = temp2.path();
        fs::write(path2.join(".mcp.json"), "{\"mcpServers\":{\"pg\":{}}}").unwrap();
        install_target(TargetId::Claude, &Location::Folder(path2.to_path_buf()), &opts);
        assert!(path2.join(".mcp.json.telemaco-backup").exists());
    }

    #[test]
    fn test_poolside_uninstall_keeps_foreign_hooks() {
        let temp = TempDir::new("poolside_uninstall");
        let path = temp.path();
        fs::create_dir_all(path.join(".poolside")).unwrap();
        let settings = path.join(".poolside").join("settings.yaml");
        let original = "# my settings\nhooks:\n  PreToolUse:\n    - name: other-guard\n      command: \"true\"\n\nmodel: gpt\n";
        fs::write(&settings, original).unwrap();

        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Poolside, &loc, &opts_with("telemaco", false, true));
        uninstall_target(TargetId::Poolside, &loc, false);

        // A full round trip has to leave the file exactly as it was: the old
        // uninstaller skipped by indentation and ate the next list item.
        assert_eq!(fs::read_to_string(&settings).unwrap(), original);
    }

    #[test]
    fn test_uninstall_keeps_a_web_guard_we_did_not_install() {
        let temp = TempDir::new("foreign_guard");
        let path = temp.path();
        fs::create_dir_all(path.join(".claude")).unwrap();
        let settings = path.join(".claude").join("settings.json");
        fs::write(
            &settings,
            r#"{"hooks":{"PreToolUse":[{"matcher":"WebSearch|WebFetch","hooks":[{"type":"command","command":"echo my own policy >&2; exit 2"}]}]}}"#,
        )
        .unwrap();

        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Claude, &loc, &opts_with("telemaco", false, true));
        uninstall_target(TargetId::Claude, &loc, false);

        let after = read_json_file(&settings);
        let groups = after["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(groups.len(), 1, "the user's own guard was removed: {:?}", groups);
        assert_eq!(groups[0]["hooks"][0]["command"], "echo my own policy >&2; exit 2");
    }

    #[test]
    fn test_pi_is_not_detected_from_an_unrelated_agents_file() {
        let temp = TempDir::new("pi_detect");
        let path = temp.path();
        // "pipeline" contains "pi"; that used to be enough.
        fs::write(path.join("AGENTS.md"), "Owns its layout and paint pipeline.\n").unwrap();

        let detected = detect_folder_targets(path);
        assert!(!detected.contains(&TargetId::Pi), "detected: {:?}", detected);

        fs::create_dir_all(path.join(".pi")).unwrap();
        assert!(detect_folder_targets(path).contains(&TargetId::Pi));
    }

    #[test]
    fn test_antigravity_is_not_detected_from_a_codegraph_index() {
        let temp = TempDir::new("agy_detect");
        let path = temp.path();
        fs::create_dir_all(path.join(".codegraph")).unwrap();

        let detected = detect_folder_targets(path);
        assert!(!detected.contains(&TargetId::Antigravity), "detected: {:?}", detected);
    }

    #[test]
    fn test_jsonc_config_keeps_other_servers() {
        // These project files are routinely edited by hand and carry comments.
        // Rewriting one as strict JSON must not drop the servers the user
        // already configured.
        let temp = TempDir::new("jsonc_mcp");
        let path = temp.path();
        fs::create_dir_all(path.join(".roo")).unwrap();
        fs::write(
            path.join(".roo").join("mcp.json"),
            "{\n  // my servers\n  \"mcpServers\": {\n    \"postgres\": {\"command\": \"pg-mcp\"},\n  }\n}\n",
        )
        .unwrap();

        let loc = Location::Folder(path.to_path_buf());
        let opts = TargetInstallOptions {
            auto_allow: false,
            stealth: false,
            binary_path: "telemaco".to_string(),
            block_builtin_web: true,
            dry_run: false,
        };
        let res = install_target(TargetId::RooCline, &loc, &opts);

        let mcp = read_json_file(&path.join(".roo").join("mcp.json"));
        assert_eq!(mcp["mcpServers"]["postgres"]["command"], "pg-mcp");
        assert!(mcp["mcpServers"]["telemaco"].is_object());
        assert!(path.join(".roo").join("mcp.json.telemaco-backup").exists());
        assert!(
            res.notes.iter().any(|n| n.contains("comments")),
            "expected a note about dropped comments, got {:?}",
            res.notes
        );
    }

    #[test]
    fn test_malformed_config_is_left_alone() {
        let temp = TempDir::new("malformed");
        let path = temp.path();
        fs::create_dir_all(path.join(".roo")).unwrap();
        let mcp_path = path.join(".roo").join("mcp.json");
        let original = "{\"mcpServers\": {\"postgres\": ";
        fs::write(&mcp_path, original).unwrap();

        let loc = Location::Folder(path.to_path_buf());
        let opts = TargetInstallOptions {
            auto_allow: false,
            stealth: false,
            binary_path: "telemaco".to_string(),
            block_builtin_web: true,
            dry_run: false,
        };
        let res = install_target(TargetId::RooCline, &loc, &opts);

        assert_eq!(fs::read_to_string(&mcp_path).unwrap(), original);
        assert!(!res.files.iter().any(|f| f.path == mcp_path));
        assert!(
            res.notes.iter().any(|n| n.contains("leaving it alone")),
            "expected a refusal note, got {:?}",
            res.notes
        );
    }

    #[test]
    fn test_hooks_use_the_resolved_binary_path() {
        // GUI-launched agents do not inherit the shell PATH, so the hook must
        // carry the same absolute binary as the MCP entry.
        let temp = TempDir::new("hook_path");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        let opts = TargetInstallOptions {
            auto_allow: false,
            stealth: false,
            binary_path: "/opt/bin/telemaco".to_string(),
            block_builtin_web: true,
            dry_run: false,
        };

        install_target(TargetId::Claude, &loc, &opts);
        let settings = read_json_file(&path.join(".claude").join("settings.json"));
        let groups = settings["hooks"]["UserPromptSubmit"].as_array().unwrap();
        assert!(groups.iter().any(|g| g["hooks"][0]["command"] == "/opt/bin/telemaco prompt-hook"));

        install_target(TargetId::Kiro, &loc, &opts);
        let hooks = read_json_file(&path.join(".kiro").join("hooks").join("telemaco.json"));
        assert_eq!(hooks["hooks"][0]["action"]["command"], "/opt/bin/telemaco prompt-hook");

        // Reinstalling from a different path rewrites in place, no duplicates.
        let opts2 = TargetInstallOptions {
            auto_allow: false,
            stealth: false,
            binary_path: "/usr/local/bin/telemaco".to_string(),
            block_builtin_web: true,
            dry_run: false,
        };
        install_target(TargetId::Claude, &loc, &opts2);
        let settings = read_json_file(&path.join(".claude").join("settings.json"));
        let groups = settings["hooks"]["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["hooks"][0]["command"], "/usr/local/bin/telemaco prompt-hook");
    }

    #[cfg(unix)]
    #[test]
    fn test_failed_write_is_reported_not_claimed() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new("readonly");
        let path = temp.path();
        let gemini_dir = path.join(".gemini");
        fs::create_dir_all(&gemini_dir).unwrap();
        fs::set_permissions(&gemini_dir, fs::Permissions::from_mode(0o555)).unwrap();

        let loc = Location::Folder(path.to_path_buf());
        let opts = TargetInstallOptions {
            auto_allow: false,
            stealth: false,
            binary_path: "telemaco".to_string(),
            block_builtin_web: true,
            dry_run: false,
        };
        let res = install_target(TargetId::Gemini, &loc, &opts);

        fs::set_permissions(&gemini_dir, fs::Permissions::from_mode(0o755)).unwrap();

        let settings_path = gemini_dir.join("settings.json");
        assert!(!settings_path.exists());
        assert!(
            !res.files.iter().any(|f| f.path == settings_path),
            "a file that could not be written must not be reported as configured"
        );
        assert!(
            res.notes.iter().any(|n| n.contains("Could not write")),
            "expected a write-failure note, got {:?}",
            res.notes
        );
    }

    #[test]
    fn test_legacy_auto_approve_key_is_migrated() {
        let temp = TempDir::new("cc_legacy_perms");
        let path = temp.path();
        fs::create_dir_all(path.join(".claude")).unwrap();
        let settings = path.join(".claude").join("settings.json");
        // What an earlier Telemaco wrote, next to a rule of the user's own.
        fs::write(
            &settings,
            "{\"autoApprove\":[\"mcp__telemaco__*\"],\"permissions\":{\"allow\":[\"Bash(npm run lint)\"]}}",
        )
        .unwrap();

        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Claude, &loc, &opts_with("telemaco", false, true));

        let json = read_json_file(&settings);
        assert!(json["autoApprove"].is_null(), "the dead key survived: {}", json);
        let allow = json["permissions"]["allow"].as_array().unwrap();
        assert!(allow.iter().any(|r| r == "mcp__telemaco__*"), "{}", json);
        assert!(allow.iter().any(|r| r == "Bash(npm run lint)"), "user rule dropped: {}", json);
    }

    #[test]
    fn test_legacy_cursor_hook_file_is_cleaned_up() {
        let temp = TempDir::new("cursor_legacy");
        let path = temp.path();
        fs::create_dir_all(path.join(".cursor")).unwrap();
        let hooks = path.join(".cursor").join("hooks.json");
        // Written by an earlier Telemaco in Claude Code's shape.
        fs::write(
            &hooks,
            "{\"hooks\":{\"UserPromptSubmit\":[{\"hooks\":[{\"type\":\"command\",\"command\":\"telemaco prompt-hook\"}]}]}}",
        )
        .unwrap();

        let loc = Location::Folder(path.to_path_buf());
        uninstall_target(TargetId::Cursor, &loc, false);
        assert!(!still_hooked(&hooks), "legacy hook left behind");
    }

    #[test]
    fn test_droid_replaces_the_legacy_wrapped_hook() {
        let temp = TempDir::new("droid_legacy");
        let path = temp.path();
        fs::create_dir_all(path.join(".factory")).unwrap();
        let hooks = path.join(".factory").join("hooks.json");
        fs::write(
            &hooks,
            "{\"hooks\":{\"UserPromptSubmit\":[{\"hooks\":[{\"type\":\"command\",\"command\":\"telemaco prompt-hook\"}]}]}}",
        )
        .unwrap();

        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Droid, &loc, &opts_with("telemaco", false, true));
        let json = read_json_file(&hooks);
        assert!(json["hooks"].is_null(), "the ignored wrapper survived: {}", json);
        assert_eq!(
            json["UserPromptSubmit"][0]["hooks"][0]["command"],
            "telemaco prompt-hook"
        );
    }

    #[test]
    fn test_poolside_web_guard_is_reversible() {
        let temp = TempDir::new("probe_pool_rev");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Poolside, &loc, &opts_with("telemaco", false, true));
        install_target(TargetId::Poolside, &loc, &opts_with("telemaco", false, false));
        let yaml = fs::read_to_string(path.join(".poolside").join("settings.yaml")).unwrap();
        assert!(!yaml.contains("telemaco-guard"), "guard survived --no-block-web:\n{}", yaml);
    }

    #[test]
    fn test_antigravity_global_uninstall_cleans_instructions() {
        let temp = TempDir::new("probe_agy_global");
        let home = temp.path().to_path_buf();
        antigravity::install(&Location::Global, &opts_with("telemaco", false, true), &home);
        let md = home.join(".gemini").join("GEMINI.md");
        assert!(md.exists(), "install wrote no instructions");
        antigravity::uninstall(&Location::Global, &home, false);
        let left = md.exists() && fs::read_to_string(&md).unwrap().contains("TELEMACO_START");
        assert!(!left, "uninstall left the block in GEMINI.md");
    }

    #[test]
    fn test_jsonc_note_never_promises_a_missing_backup() {
        let temp = TempDir::new("probe_jsonc_note");
        let path = temp.path();
        let mcp = path.join(".mcp.json");
        fs::write(&mcp, "{\n  // telemaco goes here\n  \"mcpServers\": {}\n}\n").unwrap();
        let loc = Location::Folder(path.to_path_buf());
        let res = install_target(TargetId::Claude, &loc, &opts_with("telemaco", false, true));
        let backup = path.join(".mcp.json.telemaco-backup");
        assert!(!backup.exists(), "a file that already names telemaco is not backed up");
        let jsonc_note = res
            .notes
            .iter()
            .find(|n| n.contains("JSON with comments"))
            .expect("the dropped comments have to be reported");
        assert!(
            !jsonc_note.contains("Backup:"),
            "note promises a backup that does not exist: {}",
            jsonc_note
        );

        // With a file we do back up, the note names it and the file is there.
        let other = path.join(".cursor").join("mcp.json");
        fs::create_dir_all(other.parent().unwrap()).unwrap();
        fs::write(&other, "{\n  // keep me\n  \"mcpServers\": {}\n}\n").unwrap();
        let res = install_target(TargetId::Cursor, &loc, &opts_with("telemaco", false, true));
        let note = res
            .notes
            .iter()
            .find(|n| n.contains("JSON with comments"))
            .expect("the dropped comments have to be reported");
        assert!(note.contains("Backup:"), "{}", note);
        assert!(path.join(".cursor").join("mcp.json.telemaco-backup").exists());
    }

    #[test]
    fn test_poolside_merges_into_a_crlf_file() {
        let temp = TempDir::new("probe_pool_crlf");
        let path = temp.path();
        fs::create_dir_all(path.join(".poolside")).unwrap();
        let settings = path.join(".poolside").join("settings.yaml");
        fs::write(&settings, "model: \"x\"\r\nmcp_servers:\r\n  other:\r\n    command: \"y\"\r\n").unwrap();
        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Poolside, &loc, &opts_with("telemaco", false, true));
        let yaml = fs::read_to_string(&settings).unwrap();
        assert!(yaml.contains("other:"), "user entry lost:\n{:?}", yaml);
        assert!(yaml.contains("telemaco:"), "our entry missing:\n{:?}", yaml);
        let mcp_keys = yaml.lines().filter(|l| l.trim_end() == "mcp_servers:").count();
        assert_eq!(mcp_keys, 1, "duplicate mcp_servers key:\n{:?}", yaml);
    }

    #[test]
    fn test_global_uninstall_leaves_no_husks() {
        let temp = TempDir::new("probe_global_all");
        let home = temp.path().to_path_buf();
        let loc = Location::Global;
        let opts = opts_with("telemaco", false, true);

        for &t in TargetId::all() {
            install_target_in(t, &loc, &opts, &home);
        }
        for &t in TargetId::all() {
            uninstall_target_in(t, &loc, &home, false);
        }

        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            if let Ok(rd) = fs::read_dir(dir) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.is_dir() { walk(&p, out); } else { out.push(p); }
                }
            }
        }
        let mut left = Vec::new();
        walk(&home, &mut left);
        assert!(left.is_empty(), "global uninstall left files: {:#?}", left);
    }

    #[test]
    fn test_generic_agent_files_do_not_detect_specific_agents() {
        // A repo that follows the cross-tool AGENTS.md convention and has a
        // shared .agents/ directory. Neither says which agent the user runs.
        let temp = TempDir::new("probe_generic");
        let path = temp.path();
        // Naming the agents is exactly what a repository that supports them
        // does, this one included; a substring match on the name turned every
        // such AGENTS.md into an install of that agent.
        fs::write(
            path.join("AGENTS.md"),
            "# Build\n\nRun cargo test.\n\nWorks with claude, codex, cursor, kiro, pi, \
             qwen, droid, gemini, windsurf, opencode, poolside, hermes and cline.\n",
        )
        .unwrap();
        fs::create_dir_all(path.join(".agents")).unwrap();
        fs::write(path.join(".agents").join("AGENTS.md"), "# shared\n").unwrap();

        let detected = detect_folder_targets(path);
        assert!(detected.is_empty(), "false positives: {:?}", detected);

        // The files each agent actually owns still identify it.
        fs::create_dir_all(path.join(".codex")).unwrap();
        fs::create_dir_all(path.join(".agents").join("rules")).unwrap();
        let detected = detect_folder_targets(path);
        assert!(detected.contains(&TargetId::Codex), "{:?}", detected);
        assert!(detected.contains(&TargetId::Antigravity), "{:?}", detected);
    }

    #[test]
    fn test_crlf_configs_keep_their_line_endings() {
        let temp = TempDir::new("crlf");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());

        fs::create_dir_all(path.join(".codex")).unwrap();
        fs::create_dir_all(path.join(".poolside")).unwrap();
        let toml = path.join(".codex").join("config.toml");
        let yaml = path.join(".poolside").join("settings.yaml");
        let md = path.join("AGENTS.md");
        fs::write(&toml, "model = \"gpt-5\"\r\n").unwrap();
        fs::write(&yaml, "mcp_servers:\r\n  other:\r\n    command: \"y\"\r\n").unwrap();
        fs::write(&md, "# My rules\r\n").unwrap();

        install_target(TargetId::Codex, &loc, &opts_with("telemaco", false, true));
        install_target(TargetId::Poolside, &loc, &opts_with("telemaco", false, true));

        for f in [&toml, &yaml, &md] {
            let content = fs::read_to_string(f).unwrap();
            assert!(content.contains("\r\n"), "line endings lost in {}", f.display());
            assert!(
                !content.replace("\r\n", "").contains('\n'),
                "mixed line endings in {}:\n{:?}",
                f.display(),
                content
            );
        }

        // And a reinstall still recognises its own block through the \r\n.
        let before: Vec<_> = [&toml, &yaml, &md]
            .iter()
            .map(|f| fs::read_to_string(f).unwrap())
            .collect();
        install_target(TargetId::Codex, &loc, &opts_with("telemaco", false, true));
        install_target(TargetId::Poolside, &loc, &opts_with("telemaco", false, true));
        let after: Vec<_> = [&toml, &yaml, &md]
            .iter()
            .map(|f| fs::read_to_string(f).unwrap())
            .collect();
        assert_eq!(before, after, "a reinstall rewrote a CRLF config");
    }

    #[test]
    fn test_windsurf_uses_the_preferred_rules_directory() {
        let temp = TempDir::new("ws_rules");
        let path = temp.path();
        fs::create_dir_all(path.join(".windsurf")).unwrap();
        let loc = Location::Folder(path.to_path_buf());

        fs::create_dir_all(path.join(".windsurf").join("rules")).unwrap();
        install_target(TargetId::Windsurf, &loc, &opts_with("telemaco", false, true));
        let rule = path.join(".windsurf").join("rules").join("telemaco.md");
        assert!(rule.exists(), "wrote the legacy file instead of the rules directory");
        let content = fs::read_to_string(&rule).unwrap();
        // A workspace rule declares its activation mode in frontmatter.
        assert!(content.starts_with("---\ntrigger: always_on\n---\n"), "{}", content);
        assert!(content.contains("<!-- TELEMACO_START -->"));
        assert!(!path.join(".windsurfrules").exists());

        uninstall_target(TargetId::Windsurf, &loc, false);
        assert!(!rule.exists(), "frontmatter husk left behind");

        // A project with neither rules directory gets the Devin one, since the
        // Devin Local agent is the default.
        let fresh = TempDir::new("ws_fresh");
        let fresh_loc = Location::Folder(fresh.path().to_path_buf());
        install_target(TargetId::Windsurf, &fresh_loc, &opts_with("telemaco", false, true));
        assert!(fresh.path().join(".devin").join("rules").join("telemaco.md").exists());
        assert!(!fresh.path().join(".windsurfrules").exists());

        // Without the directory, the legacy root file is still the target and
        // is merged into rather than replaced.
        let plain = TempDir::new("ws_plain");
        let loc = Location::Folder(plain.path().to_path_buf());
        fs::write(plain.path().join(".windsurfrules"), "# Mine\n").unwrap();
        install_target(TargetId::Windsurf, &loc, &opts_with("telemaco", false, true));
        let content = fs::read_to_string(plain.path().join(".windsurfrules")).unwrap();
        assert!(content.contains("# Mine"), "{}", content);
        assert!(content.contains("<!-- TELEMACO_START -->"), "{}", content);
        uninstall_target(TargetId::Windsurf, &loc, false);
        let content = fs::read_to_string(plain.path().join(".windsurfrules")).unwrap();
        assert!(!content.contains("TELEMACO_START"), "{}", content);
        assert!(content.contains("# Mine"), "{}", content);
    }

    /// Config files are routinely symlinks into a dotfiles repo. The write path
    /// has followed them since the first round; uninstall has to as well.
    #[cfg(unix)]
    #[test]
    fn test_uninstall_never_orphans_a_symlinked_config() {
        let temp = TempDir::new("symlink_uninstall");
        let path = temp.path();
        let dotfiles = path.join("dotfiles");
        let project = path.join("project");
        fs::create_dir_all(&dotfiles).unwrap();
        fs::create_dir_all(project.join(".cursor").join("rules")).unwrap();

        // A config the user owns, holding one of their servers plus ours.
        let real_mcp = dotfiles.join("cursor-mcp.json");
        fs::write(&real_mcp, "{\"mcpServers\":{}}").unwrap();
        std::os::unix::fs::symlink(&real_mcp, project.join(".cursor").join("mcp.json")).unwrap();

        let loc = Location::Folder(project.clone());
        install_target(TargetId::Cursor, &loc, &opts_with("telemaco", false, true));
        assert!(read_json_file(&real_mcp)["mcpServers"]["telemaco"].is_object());

        uninstall_target(TargetId::Cursor, &loc, false);
        let link = project.join(".cursor").join("mcp.json");
        assert!(
            fs::symlink_metadata(&link).unwrap().file_type().is_symlink(),
            "the user's symlink was replaced or deleted"
        );
        assert!(
            read_json_file(&real_mcp)["mcpServers"]["telemaco"].is_null(),
            "our entry survived in the file the link points at"
        );

        // A file we own outright takes its target with it.
        let real_rule = dotfiles.join("telemaco.mdc");
        fs::write(&real_rule, "x").unwrap();
        let link_rule = project.join(".cursor").join("rules").join("telemaco.mdc");
        let _ = fs::remove_file(&link_rule);
        std::os::unix::fs::symlink(&real_rule, &link_rule).unwrap();
        uninstall_target(TargetId::Cursor, &loc, false);
        assert!(!link_rule.exists(), "link left behind");
        assert!(!real_rule.exists(), "the file the link pointed at was orphaned");
    }

    /// The symlink rule has to be the same everywhere: a config the user owns
    /// is emptied, never unlinked, whatever format it is in.
    #[cfg(unix)]
    #[test]
    fn test_symlinked_user_config_is_emptied_not_unlinked() {
        let temp = TempDir::new("probe_symlink_fmt");
        let path = temp.path();
        let dotfiles = path.join("dotfiles");
        let project = path.join("project");
        fs::create_dir_all(&dotfiles).unwrap();
        fs::create_dir_all(project.join(".poolside")).unwrap();
        fs::create_dir_all(project.join(".codex")).unwrap();

        let real_yaml = dotfiles.join("poolside.yaml");
        let real_toml = dotfiles.join("codex.toml");
        fs::write(&real_yaml, "").unwrap();
        fs::write(&real_toml, "").unwrap();
        let link_yaml = project.join(".poolside").join("settings.yaml");
        let link_toml = project.join(".codex").join("config.toml");
        std::os::unix::fs::symlink(&real_yaml, &link_yaml).unwrap();
        std::os::unix::fs::symlink(&real_toml, &link_toml).unwrap();

        let loc = Location::Folder(project.clone());
        install_target(TargetId::Poolside, &loc, &opts_with("telemaco", false, true));
        install_target(TargetId::Codex, &loc, &opts_with("telemaco", false, true));
        uninstall_target(TargetId::Poolside, &loc, false);
        uninstall_target(TargetId::Codex, &loc, false);

        for (link, real) in [(&link_yaml, &real_yaml), (&link_toml, &real_toml)] {
            assert!(
                fs::symlink_metadata(link).map_or(false, |m| m.file_type().is_symlink()),
                "the user's symlink was deleted: {}",
                link.display()
            );
            assert!(real.exists(), "the dotfiles file was deleted: {}", real.display());
        }
    }

    #[test]
    fn test_one_file_is_reported_once_per_install() {
        let temp = TempDir::new("probe_dupe");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        for &t in TargetId::all() {
            let res = install_target(t, &loc, &opts_with("telemaco", false, true));
            let mut seen: Vec<String> = res
                .files
                .iter()
                .map(|f| f.path.display().to_string())
                .collect();
            let before = seen.len();
            seen.sort();
            seen.dedup();
            assert_eq!(before, seen.len(), "{} reported a file twice: {:?}", t.id_str(), res.files.iter().map(|f| f.path.display().to_string()).collect::<Vec<_>>());
        }
    }

    #[test]
    fn test_devin_directory_is_detected() {
        let temp = TempDir::new("probe_devin");
        let path = temp.path();
        fs::create_dir_all(path.join(".devin").join("rules")).unwrap();
        let detected = detect_folder_targets(path);
        assert!(detected.contains(&TargetId::Windsurf), "detected: {:?}", detected);
    }

    #[test]
    fn test_every_target_keeps_keys_the_user_added_to_our_entry() {
        // The round-4 merge fix has to hold for every target, not just the ones
        // that go through upsert_mcp_server.
        let cases: &[(TargetId, &str, &str)] = &[
            (TargetId::QwenCode, ".qwen/settings.json", "mcpServers"),
            (TargetId::Gemini, ".gemini/settings.json", "mcpServers"),
            (TargetId::Claude, ".mcp.json", "mcpServers"),
            (TargetId::Cursor, ".cursor/mcp.json", "mcpServers"),
            (TargetId::OpenCode, "opencode.json", "mcp"),
        ];
        for (target, rel, container) in cases {
            let temp = TempDir::new("probe_merge");
            let path = temp.path();
            let file = path.join(rel);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(
                &file,
                format!(
                    "{{\"{}\":{{\"telemaco\":{{\"command\":\"old\",\"env\":{{\"HTTPS_PROXY\":\"http://corp:8080\"}}}}}}}}",
                    container
                ),
            )
            .unwrap();

            let loc = Location::Folder(path.to_path_buf());
            install_target(*target, &loc, &opts_with("/opt/bin/telemaco", false, true));

            let json = read_json_file(&file);
            let entry = &json[*container]["telemaco"];
            assert_eq!(entry["env"]["HTTPS_PROXY"], "http://corp:8080", "{} dropped the user's env: {}", target.id_str(), json);
        }
    }

    #[test]
    fn test_antigravity_hook_entry_keeps_user_keys() {
        let temp = TempDir::new("probe_agy_merge");
        let path = temp.path();
        fs::create_dir_all(path.join(".agents")).unwrap();
        // The user disabled our hook and added an event of their own to it.
        fs::write(
            path.join(".agents").join("hooks.json"),
            "{\"telemaco\":{\"enabled\":false,\"PreInvocation\":[{\"command\":\"old\",\"type\":\"command\"}],\"Stop\":[{\"command\":\"mine.sh\",\"type\":\"command\"}]}}",
        )
        .unwrap();

        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Antigravity, &loc, &opts_with("telemaco", false, true));

        let json = read_json_file(&path.join(".agents").join("hooks.json"));
        assert_eq!(json["telemaco"]["PreInvocation"][0]["command"], "telemaco prompt-hook");
        assert_eq!(json["telemaco"]["Stop"][0]["command"], "mine.sh", "dropped a foreign event: {}", json);
        assert_eq!(json["telemaco"]["enabled"], false, "re-enabled a hook the user turned off: {}", json);
    }

    #[test]
    fn test_reinstall_updates_every_stale_hook_group() {
        // Two groups of ours, as an older buggy install could leave behind.
        let mut cfg = serde_json::json!({
            "hooks": { "UserPromptSubmit": [
                {"hooks": [{"type": "command", "command": "/old/a/telemaco prompt-hook"}]},
                {"hooks": [{"type": "command", "command": "/old/b/telemaco prompt-hook"}]}
            ]}
        });
        common::add_user_prompt_hook(&mut cfg, "/new/telemaco prompt-hook");
        let groups = cfg["hooks"]["UserPromptSubmit"].as_array().unwrap();
        let stale: Vec<_> = groups
            .iter()
            .filter(|g| g["hooks"][0]["command"] != "/new/telemaco prompt-hook")
            .collect();
        assert!(stale.is_empty(), "a stale hook still points at a binary that moved: {:?}", stale);
    }

    #[test]
    fn test_hermes_global_writes_servers_and_hook_in_one_file() {
        let temp = TempDir::new("hermes_global");
        let home = temp.path().to_path_buf();
        let cfg = home.join(".hermes").join("config.yaml");

        hermes::install(&Location::Global, &opts_with("telemaco", false, true), &home);
        let yaml = fs::read_to_string(&cfg).unwrap();
        assert!(yaml.contains("mcp_servers:"), "{}", yaml);
        assert!(yaml.contains("  telemaco:"), "{}", yaml);
        assert!(yaml.contains("pre_llm_call:"), "{}", yaml);
        assert!(yaml.contains("--format hermes"), "{}", yaml);

        hermes::uninstall(&Location::Global, &home, false);
        assert!(!cfg.exists(), "left a husk: {:?}", fs::read_to_string(&cfg).ok());
    }

    #[test]
    fn test_hermes_keeps_the_users_yaml() {
        let temp = TempDir::new("hermes_merge");
        let home = temp.path().to_path_buf();
        fs::create_dir_all(home.join(".hermes")).unwrap();
        let cfg = home.join(".hermes").join("config.yaml");
        fs::write(
            &cfg,
            "model: hermes-4\n\nmcp_servers:\n  github:\n    command: \"npx\"\n\nhooks:\n  pre_tool_call:\n    - command: \"./guard.sh\"\n",
        )
        .unwrap();

        hermes::install(&Location::Global, &opts_with("/opt/bin/telemaco", false, true), &home);
        let yaml = fs::read_to_string(&cfg).unwrap();
        assert!(yaml.contains("model: hermes-4"), "{}", yaml);
        assert!(yaml.contains("github:"), "{}", yaml);
        assert!(yaml.contains("./guard.sh"), "{}", yaml);
        assert!(yaml.contains("/opt/bin/telemaco prompt-hook --format hermes"), "{}", yaml);
        assert_eq!(yaml.matches("mcp_servers:").count(), 1, "{}", yaml);
        assert_eq!(yaml.matches("hooks:").count(), 1, "{}", yaml);

        // A moved binary is followed, not left stale.
        hermes::install(&Location::Global, &opts_with("/new/telemaco", false, true), &home);
        let yaml = fs::read_to_string(&cfg).unwrap();
        assert!(!yaml.contains("/opt/bin/telemaco"), "{}", yaml);
        assert_eq!(yaml.matches("--format hermes").count(), 1, "{}", yaml);

        hermes::uninstall(&Location::Global, &home, false);
        let yaml = fs::read_to_string(&cfg).unwrap();
        assert!(yaml.contains("model: hermes-4"), "{}", yaml);
        assert!(yaml.contains("github:"), "{}", yaml);
        assert!(yaml.contains("./guard.sh"), "{}", yaml);
        assert!(!yaml.contains("telemaco prompt-hook"), "{}", yaml);
    }

    #[test]
    fn test_hermes_writes_the_context_file_it_would_read() {
        // Hermes scans .hermes.md, AGENTS.md, CLAUDE.md, .cursorrules and stops
        // at the first one it finds.
        let temp = TempDir::new("hermes_ctx");
        let path = temp.path();
        fs::write(path.join(".hermes.md"), "# Mine\n").unwrap();
        fs::write(path.join("AGENTS.md"), "# Shared\n").unwrap();

        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Hermes, &loc, &opts_with("telemaco", false, true));

        let own = fs::read_to_string(path.join(".hermes.md")).unwrap();
        assert!(own.contains("TELEMACO_START"), "{}", own);
        let shared = fs::read_to_string(path.join("AGENTS.md")).unwrap();
        assert!(!shared.contains("TELEMACO_START"), "wrote a file Hermes skips: {}", shared);

        uninstall_target(TargetId::Hermes, &loc, false);
        let own = fs::read_to_string(path.join(".hermes.md")).unwrap();
        assert!(!own.contains("TELEMACO_START"), "{}", own);
        assert!(own.contains("# Mine"), "{}", own);
    }

    #[test]
    fn test_hermes_is_not_detected_from_a_shared_agents_file() {
        let temp = TempDir::new("hermes_detect");
        let path = temp.path();
        fs::write(path.join("AGENTS.md"), "# Shared\n").unwrap();
        let detected = detect_folder_targets(path);
        assert!(!detected.contains(&TargetId::Hermes), "detected: {:?}", detected);
    }

    #[test]
    fn test_windsurf_installs_the_devin_prompt_hook() {
        // The Devin CLI, Windsurf's default agent, runs UserPromptSubmit hooks.
        // A project's standalone file is the event map itself, with no wrapper.
        let temp = TempDir::new("ws_hook");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Windsurf, &loc, &opts_with("telemaco", false, true));

        let hooks = read_json_file(&path.join(".devin").join("hooks.v1.json"));
        assert!(hooks.get("hooks").is_none(), "wrapped a standalone file: {}", hooks);
        let cmd = &hooks["UserPromptSubmit"][0]["hooks"][0]["command"];
        assert_eq!(cmd, "telemaco prompt-hook --format json", "{}", hooks);

        uninstall_target(TargetId::Windsurf, &loc, false);
        assert!(!path.join(".devin").join("hooks.v1.json").exists(), "left a husk");
    }

    #[test]
    fn test_windsurf_global_hook_nests_under_the_config_key() {
        // Every location other than the standalone file nests hooks under
        // `hooks`, and the user-level one is the Devin config file.
        let temp = TempDir::new("ws_hook_global");
        let home = temp.path().to_path_buf();
        fs::create_dir_all(home.join(".config").join("devin")).unwrap();
        fs::write(
            home.join(".config").join("devin").join("config.json"),
            "{\"theme\":\"dark\"}",
        )
        .unwrap();

        windsurf::install(&Location::Global, &opts_with("telemaco", false, true), &home);
        let cfg = read_json_file(&home.join(".config").join("devin").join("config.json"));
        assert_eq!(
            cfg["hooks"]["UserPromptSubmit"][0]["hooks"][0]["command"],
            "telemaco prompt-hook --format json",
            "{}",
            cfg
        );
        assert_eq!(cfg["theme"], "dark", "dropped a setting of the user's");

        // And the Devin CLI gets a global rule of its own, with its trigger.
        let rule = fs::read_to_string(home.join(".devin").join("rules").join("telemaco.md")).unwrap();
        assert!(rule.starts_with("---\ntrigger: always_on\n---\n"), "{}", rule);

        windsurf::uninstall(&Location::Global, &home, false);
        let cfg = read_json_file(&home.join(".config").join("devin").join("config.json"));
        assert!(cfg["hooks"].is_null(), "{}", cfg);
        assert_eq!(cfg["theme"], "dark");
        assert!(!home.join(".devin").join("rules").join("telemaco.md").exists());
    }

    #[test]
    fn test_cursor_global_install_uses_the_session_hook() {
        // Cursor's global instructions are User Rules, typed into the Customize
        // panel. `~/.cursor/rules/` is not a documented path, so the directive
        // travels through the documented user-level sessionStart hook instead.
        let temp = TempDir::new("cursor_global");
        let home = temp.path().to_path_buf();
        cursor::install(&Location::Global, &opts_with("telemaco", false, true), &home);

        let hooks = read_json_file(&home.join(".cursor").join("hooks.json"));
        assert_eq!(hooks["version"], 1, "{}", hooks);
        let entries = hooks["hooks"]["sessionStart"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["command"], "telemaco prompt-hook --format cursor");
        assert!(
            !home.join(".cursor").join("rules").join("telemaco.mdc").exists(),
            "wrote a rule where Cursor does not look"
        );

        cursor::uninstall(&Location::Global, &home, false);
        assert!(!home.join(".cursor").join("hooks.json").exists(), "left a husk");
    }

    #[test]
    fn test_cursor_session_hook_keeps_the_users_own_hooks() {
        let temp = TempDir::new("cursor_hooks_merge");
        let home = temp.path().to_path_buf();
        fs::create_dir_all(home.join(".cursor")).unwrap();
        fs::write(
            home.join(".cursor").join("hooks.json"),
            "{\"version\":1,\"hooks\":{\"sessionStart\":[{\"command\":\"./mine.sh\"}],\"afterFileEdit\":[{\"command\":\"./fmt.sh\"}]}}",
        )
        .unwrap();

        cursor::install(&Location::Global, &opts_with("/opt/bin/telemaco", false, true), &home);
        let hooks = read_json_file(&home.join(".cursor").join("hooks.json"));
        let starts = hooks["hooks"]["sessionStart"].as_array().unwrap();
        assert_eq!(starts.len(), 2, "{}", hooks);
        assert_eq!(starts[0]["command"], "./mine.sh");
        assert_eq!(hooks["hooks"]["afterFileEdit"][0]["command"], "./fmt.sh");

        cursor::uninstall(&Location::Global, &home, false);
        let hooks = read_json_file(&home.join(".cursor").join("hooks.json"));
        assert_eq!(hooks["hooks"]["sessionStart"].as_array().unwrap().len(), 1, "{}", hooks);
        assert_eq!(hooks["hooks"]["sessionStart"][0]["command"], "./mine.sh");
        assert_eq!(hooks["hooks"]["afterFileEdit"][0]["command"], "./fmt.sh");
    }

    #[test]
    fn test_cursor_mcp_entry_declares_its_transport() {
        // `type` is required for a STDIO server in Cursor's field table.
        let temp = TempDir::new("cursor_type");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Cursor, &loc, &opts_with("telemaco", false, true));
        let json = read_json_file(&path.join(".cursor").join("mcp.json"));
        assert_eq!(json["mcpServers"]["telemaco"]["type"], "stdio", "{}", json);
    }

    #[test]
    fn test_opencode_is_not_detected_from_a_stray_markdown_file() {
        // OpenCode reads AGENTS.md, with CLAUDE.md as a fallback. Nothing it
        // documents is called OPENCODE.md.
        let temp = TempDir::new("oc_stray");
        let path = temp.path();
        fs::write(path.join("OPENCODE.md"), "# notes\n").unwrap();
        let detected = detect_folder_targets(path);
        assert!(!detected.contains(&TargetId::OpenCode), "detected: {:?}", detected);
    }

    #[test]
    fn test_opencode_writes_into_an_existing_jsonc_config() {
        // A project that keeps its config as JSONC gets its own file updated,
        // not a second opencode.json next to it.
        let temp = TempDir::new("oc_jsonc");
        let path = temp.path();
        fs::write(
            path.join("opencode.jsonc"),
            "{\n  // mine\n  \"model\": \"anthropic/claude-sonnet-5\"\n}\n",
        )
        .unwrap();

        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::OpenCode, &loc, &opts_with("telemaco", false, true));
        assert!(!path.join("opencode.json").exists(), "wrote a second config file");
        let json = read_json_file(&path.join("opencode.jsonc"));
        assert_eq!(json["mcp"]["telemaco"]["type"], "local");
        assert_eq!(json["model"], "anthropic/claude-sonnet-5", "dropped the user's key");
    }

    #[test]
    fn test_antigravity_covers_the_cli_and_the_app() {
        // The IDE reads workspace rules from `.agents/rules`; the CLI reads the
        // project's AGENTS.md. Both surfaces need the directive.
        let temp = TempDir::new("agy_surfaces");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Antigravity, &loc, &opts_with("telemaco", false, true));

        let rule = fs::read_to_string(path.join(".agents").join("rules").join("telemaco.md")).unwrap();
        assert!(rule.contains("TELEMACO_START"), "{}", rule);
        let agents = fs::read_to_string(path.join("AGENTS.md")).unwrap();
        assert!(agents.contains("TELEMACO_START"), "the CLI reads this one: {}", agents);

        uninstall_target(TargetId::Antigravity, &loc, false);
        assert!(!path.join("AGENTS.md").exists(), "left the block behind");
        assert!(!path.join(".agents").join("rules").join("telemaco.md").exists());
    }

    #[test]
    fn test_antigravity_cli_only_machine_is_detected() {
        // A machine with the CLI and not the app has ~/.gemini/antigravity-cli
        // and no ~/.gemini/antigravity.
        let temp = TempDir::new("agy_cli_only");
        let home = temp.path().to_path_buf();
        fs::create_dir_all(home.join(".gemini").join("antigravity-cli")).unwrap();
        let det = antigravity::detect(&Location::Global, Some(&home));
        assert!(det.installed, "the CLI is an Antigravity install too");
    }

    #[test]
    fn test_codex_writes_to_the_override_file_when_there_is_one() {
        // Codex reads AGENTS.override.md and stops there: with one present,
        // everything written to AGENTS.md is never loaded.
        let temp = TempDir::new("codex_override");
        let path = temp.path();
        fs::write(path.join("AGENTS.override.md"), "# My override\n").unwrap();
        fs::write(path.join("AGENTS.md"), "# Shared\n").unwrap();

        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Codex, &loc, &opts_with("telemaco", false, true));

        let over = fs::read_to_string(path.join("AGENTS.override.md")).unwrap();
        assert!(over.contains("TELEMACO_START"), "{}", over);
        assert!(over.contains("# My override"), "{}", over);
        let shared = fs::read_to_string(path.join("AGENTS.md")).unwrap();
        assert!(!shared.contains("TELEMACO_START"), "wrote where Codex does not look: {}", shared);

        uninstall_target(TargetId::Codex, &loc, false);
        let over = fs::read_to_string(path.join("AGENTS.override.md")).unwrap();
        assert!(!over.contains("TELEMACO_START"), "{}", over);
        assert!(over.contains("# My override"), "{}", over);
    }

    #[test]
    fn test_codex_global_override_file_wins_too() {
        let temp = TempDir::new("codex_override_global");
        let home = temp.path().to_path_buf();
        fs::create_dir_all(home.join(".codex")).unwrap();
        fs::write(home.join(".codex").join("AGENTS.override.md"), "# Mine\n").unwrap();

        codex::install(&Location::Global, &opts_with("telemaco", false, true), &home);
        let over = fs::read_to_string(home.join(".codex").join("AGENTS.override.md")).unwrap();
        assert!(over.contains("TELEMACO_START"), "{}", over);
        assert!(!home.join(".codex").join("AGENTS.md").exists(), "wrote the file Codex ignores");
    }

    #[test]
    fn test_codex_flags_an_inline_hooks_table() {
        // Two hook representations in one layer make Codex warn at startup.
        let temp = TempDir::new("codex_inline_hooks");
        let path = temp.path();
        fs::create_dir_all(path.join(".codex")).unwrap();
        fs::write(
            path.join(".codex").join("config.toml"),
            "[[hooks.PreToolUse]]\nmatcher = \"^Bash$\"\n",
        )
        .unwrap();

        let loc = Location::Folder(path.to_path_buf());
        let res = install_target(TargetId::Codex, &loc, &opts_with("telemaco", false, true));
        assert!(
            res.notes.iter().any(|n| n.contains("declares hooks inline")),
            "no warning about the merge, notes: {:?}",
            res.notes
        );
    }

    #[test]
    fn test_codex_is_not_detected_from_a_stray_codex_toml() {
        // Codex reads .codex/config.toml, ~/.codex/config.toml and the system
        // file. A codex.toml at a repository root is nobody's config.
        let temp = TempDir::new("codex_stray");
        let path = temp.path();
        fs::write(path.join("codex.toml"), "model = \"x\"\n").unwrap();
        let detected = detect_folder_targets(path);
        assert!(!detected.contains(&TargetId::Codex), "detected: {:?}", detected);
    }

    #[test]
    fn test_claude_project_server_is_approved() {
        // Claude Code does not connect a server from `.mcp.json` until it is
        // approved, so the install has to leave the approval it would have
        // asked for, in the personal file Claude Code writes it to.
        let temp = TempDir::new("claude_approval");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        let local = path.join(".claude").join("settings.local.json");

        install_target(TargetId::Claude, &loc, &opts_with("telemaco", false, true));
        let json = read_json_file(&local);
        assert_eq!(
            json["enabledMcpjsonServers"],
            serde_json::json!(["telemaco"]),
            "server left pending approval: {}",
            json
        );

        uninstall_target(TargetId::Claude, &loc, false);
        assert!(!local.exists(), "left an approval for a server that is gone");
    }

    #[test]
    fn test_claude_approval_follows_no_permissions() {
        let temp = TempDir::new("claude_noapproval");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        let local = path.join(".claude").join("settings.local.json");

        // Granted once...
        install_target(TargetId::Claude, &loc, &opts_with("telemaco", false, true));
        assert!(local.exists());

        // ...then declined: the grant goes with it, as `permissions.allow` does.
        let opts = TargetInstallOptions {
            auto_allow: false,
            stealth: true,
            binary_path: "telemaco".to_string(),
            block_builtin_web: true,
            dry_run: false,
        };
        install_target(TargetId::Claude, &loc, &opts);
        assert!(
            !local.exists() || read_json_file(&local)["enabledMcpjsonServers"].is_null(),
            "kept an approval --no-permissions revoked"
        );
    }

    #[test]
    fn test_claude_approval_keeps_the_users_other_entries() {
        let temp = TempDir::new("claude_approval_merge");
        let path = temp.path();
        fs::create_dir_all(path.join(".claude")).unwrap();
        let local = path.join(".claude").join("settings.local.json");
        fs::write(
            &local,
            "{\"enabledMcpjsonServers\":[\"memory\"],\"model\":\"opus\"}",
        )
        .unwrap();

        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Claude, &loc, &opts_with("telemaco", false, true));
        let json = read_json_file(&local);
        assert_eq!(json["enabledMcpjsonServers"], serde_json::json!(["memory", "telemaco"]));

        uninstall_target(TargetId::Claude, &loc, false);
        let json = read_json_file(&local);
        assert_eq!(json["enabledMcpjsonServers"], serde_json::json!(["memory"]), "{}", json);
        assert_eq!(json["model"], "opus", "dropped a setting of the user's: {}", json);
    }

    #[test]
    fn test_claude_writes_where_the_project_keeps_its_instructions() {
        // `./CLAUDE.md` and `./.claude/CLAUDE.md` are both project memory. A
        // project already using the nested one gets no second file at its root.
        let temp = TempDir::new("claude_nested_md");
        let path = temp.path();
        fs::create_dir_all(path.join(".claude")).unwrap();
        fs::write(path.join(".claude").join("CLAUDE.md"), "# Team rules\n").unwrap();

        let loc = Location::Folder(path.to_path_buf());
        install_target(TargetId::Claude, &loc, &opts_with("telemaco", false, true));

        let nested = fs::read_to_string(path.join(".claude").join("CLAUDE.md")).unwrap();
        assert!(nested.contains("TELEMACO_START"), "{}", nested);
        assert!(nested.contains("# Team rules"), "{}", nested);
        assert!(!path.join("CLAUDE.md").exists(), "wrote a second instructions file");

        uninstall_target(TargetId::Claude, &loc, false);
        let nested = fs::read_to_string(path.join(".claude").join("CLAUDE.md")).unwrap();
        assert!(!nested.contains("TELEMACO_START"), "{}", nested);
        assert!(nested.contains("# Team rules"), "{}", nested);
    }

    #[test]
    fn test_windsurf_global_covers_both_agents() {
        // Cascade reads `~/.codeium/windsurf/mcp_config.json`; the Devin Local
        // agent, the default for new tabs, reads `~/.config/devin/`. A global
        // install has to reach both, and take both back out.
        let temp = TempDir::new("ws_global");
        let home = temp.path().to_path_buf();
        let cascade = home.join(".codeium").join("windsurf").join("mcp_config.json");
        let devin = home.join(".config").join("devin").join("mcp_config.json");

        windsurf::install(&Location::Global, &opts_with("telemaco", false, true), &home);
        for path in [&cascade, &devin] {
            let json = read_json_file(path);
            assert_eq!(json["mcpServers"]["telemaco"]["command"], "telemaco", "{}", path.display());
        }

        windsurf::uninstall(&Location::Global, &home, false);
        for path in [&cascade, &devin] {
            assert!(
                !path.exists() || read_json_file(path)["mcpServers"]["telemaco"].is_null(),
                "{} still names us",
                path.display()
            );
        }
    }

    #[test]
    fn test_folder_install_and_uninstall_kiro() {
        let temp = TempDir::new("kiro_install");
        let path = temp.path();
        let loc = Location::Folder(path.to_path_buf());
        let opts = TargetInstallOptions {
            auto_allow: true,
            stealth: true,
            binary_path: "telemaco".to_string(),
            block_builtin_web: true,
            dry_run: false,
        };

        install_target(TargetId::Kiro, &loc, &opts);
        assert!(path.join(".kiro").join("settings").join("mcp.json").exists());
        assert!(path.join(".kiro").join("hooks").join("telemaco.json").exists());
        assert!(path.join("AGENTS.md").exists());

        let mcp = read_json_file(&path.join(".kiro").join("settings").join("mcp.json"));
        assert!(mcp["mcpServers"]["telemaco"].is_object());
        assert_eq!(mcp["mcpServers"]["telemaco"]["autoApprove"], serde_json::json!(["*"]));

        let hooks = read_json_file(&path.join(".kiro").join("hooks").join("telemaco.json"));
        assert!(hooks["hooks"].as_array().unwrap().iter().any(|h| h["action"]["command"] == "telemaco prompt-hook"));

        let det = detect_target(TargetId::Kiro, &loc);
        assert!(det.installed);
        assert!(det.already_configured);

        uninstall_target(TargetId::Kiro, &loc, false);
        let mcp_after = read_json_file(&path.join(".kiro").join("settings").join("mcp.json"));
        assert!(mcp_after["mcpServers"]["telemaco"].is_null());
        assert!(!path.join(".kiro").join("hooks").join("telemaco.json").exists());
    }

    // Every target below reads its documented home-directory override
    // (`CODEX_HOME`, `CLAUDE_CONFIG_DIR`, ...) through `home_env_var`, which
    // in a test build looks at a `TELEMACO_TEST_`-prefixed name instead of
    // the real one. These tests set only that fake name, via `FakeHomeVar`,
    // and assert install/detect follow it instead of the explicit `home`
    // passed in - proving the ambient real variable (set in a developer's
    // own shell, say) can no longer reach here at all.

    #[test]
    fn test_codex_home_env_override_wins_over_explicit_home() {
        let real_home = TempDir::new("codex_env_real_home");
        let fake = TempDir::new("codex_env_fake");
        let _var = FakeHomeVar::set("CODEX_HOME", fake.path());
        let real_home = real_home.path().to_path_buf();

        install_target_in(TargetId::Codex, &Location::Global, &opts_with("telemaco", false, true), &real_home);

        let toml = fs::read_to_string(fake.path().join("config.toml")).unwrap();
        assert!(toml.contains("[mcp_servers.telemaco]"), "{}", toml);
        assert!(!real_home.join(".codex").exists());

        let det = detect_target_in(TargetId::Codex, &Location::Global, Some(&real_home));
        assert!(det.installed);
        assert!(det.already_configured);
    }

    #[test]
    fn test_hermes_home_env_override_wins_over_explicit_home() {
        let real_home = TempDir::new("hermes_env_real_home");
        let fake = TempDir::new("hermes_env_fake");
        let _var = FakeHomeVar::set("HERMES_HOME", fake.path());
        let real_home = real_home.path().to_path_buf();

        install_target_in(TargetId::Hermes, &Location::Global, &opts_with("telemaco", false, true), &real_home);

        assert!(fake.path().join("config.yaml").exists());
        assert!(!real_home.join(".hermes").exists());

        let det = detect_target_in(TargetId::Hermes, &Location::Global, Some(&real_home));
        assert!(det.installed);
        assert!(det.already_configured);
    }

    #[test]
    fn test_gemini_home_env_override_wins_over_explicit_home() {
        let real_home = TempDir::new("gemini_env_real_home");
        let fake = TempDir::new("gemini_env_fake");
        let _var = FakeHomeVar::set("GEMINI_CLI_HOME", fake.path());
        let real_home = real_home.path().to_path_buf();

        install_target_in(TargetId::Gemini, &Location::Global, &opts_with("telemaco", false, true), &real_home);

        assert!(fake.path().join(".gemini").join("settings.json").exists());
        assert!(!real_home.join(".gemini").exists());

        let det = detect_target_in(TargetId::Gemini, &Location::Global, Some(&real_home));
        assert!(det.installed);
        assert!(det.already_configured);
    }

    #[test]
    fn test_pi_home_env_override_wins_over_explicit_home() {
        let real_home = TempDir::new("pi_env_real_home");
        let fake = TempDir::new("pi_env_fake");
        let _var = FakeHomeVar::set("PI_CODING_AGENT_DIR", fake.path());
        let real_home = real_home.path().to_path_buf();

        install_target_in(TargetId::Pi, &Location::Global, &opts_with("telemaco", false, true), &real_home);

        assert!(fake.path().join("AGENTS.md").exists());
        assert!(!real_home.join(".pi").exists());

        let det = detect_target_in(TargetId::Pi, &Location::Global, Some(&real_home));
        assert!(det.installed);
        assert!(det.already_configured);
    }

    #[test]
    fn test_deepseek_home_env_override_wins_over_explicit_home() {
        let real_home = TempDir::new("dsh_env_real_home");
        let fake = TempDir::new("dsh_env_fake");
        let _var = FakeHomeVar::set("DSH_HOME", fake.path());
        let real_home = real_home.path().to_path_buf();

        install_target_in(TargetId::DeepSeek, &Location::Global, &opts_with("telemaco", false, true), &real_home);

        let patch = fs::read_to_string(fake.path().join("cordis.patch.yml")).unwrap();
        assert!(patch.contains("id: telemaco-mcp"), "{}", patch);
        assert!(!real_home.join(".dsh").exists());

        let det = detect_target_in(TargetId::DeepSeek, &Location::Global, Some(&real_home));
        assert!(det.installed);
        assert!(det.already_configured);
    }

    #[test]
    fn test_claude_config_dir_env_override_wins_over_explicit_home() {
        let real_home = TempDir::new("claude_env_real_home");
        let fake = TempDir::new("claude_env_fake");
        let _var = FakeHomeVar::set("CLAUDE_CONFIG_DIR", fake.path());
        let real_home = real_home.path().to_path_buf();

        install_target_in(TargetId::Claude, &Location::Global, &opts_with("telemaco", false, true), &real_home);

        // Flat, directly inside the override - not nested under a further
        // `.claude/`, matching a real relocated Claude Code install.
        assert!(fake.path().join("settings.json").exists());
        assert!(fake.path().join("CLAUDE.md").exists());
        assert!(fake.path().join(".claude.json").exists());
        assert!(!real_home.join(".claude").exists());
        assert!(!real_home.join(".claude.json").exists());

        let det = detect_target_in(TargetId::Claude, &Location::Global, Some(&real_home));
        assert!(det.installed);
        assert!(det.already_configured);
    }
}
