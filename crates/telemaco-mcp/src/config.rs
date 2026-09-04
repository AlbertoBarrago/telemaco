//! Configurable extraction limits for the MCP server.
//!
//! Every cap an MCP tool applies to its own output is resolved here, from four
//! layers, in this order of precedence:
//!
//! ```text
//! tool call argument > CLI flag > environment variable > config file > default
//! ```
//!
//! The tool-call argument is applied per call by each tool. Everything below it
//! is resolved once at startup into an [`ExtractionLimits`] carried on
//! `BrowserState`, which every tool already receives by `&mut`.
//!
//! A limit of `0` means unlimited.
//!
//! The caps exist because MCP output lands directly in an agent's context
//! window: an uncapped dump of a large page can burn a whole window in one
//! call. They are a context budget, not an arbitrary restriction, which is why
//! raising them is a deliberate act of configuration.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

pub const DEFAULT_MAX_CHARS: usize = 4_000;
pub const DEFAULT_MAX_LINKS: usize = 100;
pub const DEFAULT_MAX_INTERACTIVE: usize = 100;
pub const DEFAULT_MAX_SEARCH_RESULTS: usize = 10;
pub const DEFAULT_SEARCH_CONTEXT_CHARS: usize = 80;
pub const DEFAULT_MAX_NETWORK_REQUESTS: usize = 500;
pub const DEFAULT_MAX_CONSOLE_MESSAGES: usize = 500;
pub const DEFAULT_MAX_FORMS: usize = 100;

/// Resolved output caps for every MCP tool that can emit unbounded text.
///
/// `deny_unknown_fields` is deliberate: a misspelled key must fail loudly at
/// startup rather than be silently dropped, leaving the operator to wonder why
/// their setting had no effect.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExtractionLimits {
    /// Characters of page text returned by `browser_markdown` / `browser_snapshot`.
    pub max_chars: usize,
    /// Anchors returned by `browser_links`.
    pub max_links: usize,
    /// Elements returned by `browser_interactive_elements`.
    pub max_interactive: usize,
    /// Matches returned by `browser_search`.
    pub max_search_results: usize,
    /// Characters of surrounding context per `browser_search` match.
    pub search_context_chars: usize,
    /// Entries returned by `browser_network_requests`.
    pub max_network_requests: usize,
    /// Entries returned by `browser_console_messages`.
    pub max_console_messages: usize,
    /// Forms returned by `browser_detect_forms`.
    pub max_forms: usize,
}

impl Default for ExtractionLimits {
    fn default() -> Self {
        ExtractionLimits {
            max_chars: DEFAULT_MAX_CHARS,
            max_links: DEFAULT_MAX_LINKS,
            max_interactive: DEFAULT_MAX_INTERACTIVE,
            max_search_results: DEFAULT_MAX_SEARCH_RESULTS,
            search_context_chars: DEFAULT_SEARCH_CONTEXT_CHARS,
            max_network_requests: DEFAULT_MAX_NETWORK_REQUESTS,
            max_console_messages: DEFAULT_MAX_CONSOLE_MESSAGES,
            max_forms: DEFAULT_MAX_FORMS,
        }
    }
}

impl ExtractionLimits {
    /// Turn a configured limit into a comparable ceiling. `0` means unlimited,
    /// so it maps to `usize::MAX` and every `take`/`>` at a call site keeps
    /// working without a special case.
    pub fn cap(value: usize) -> usize {
        if value == 0 {
            usize::MAX
        } else {
            value
        }
    }

    /// Per-call override: a tool argument wins over the resolved config, and
    /// `0` from a caller means unlimited just as it does in the file.
    pub fn override_with(configured: usize, arg: Option<u64>) -> usize {
        Self::cap(arg.map(|n| n as usize).unwrap_or(configured))
    }
}

/// Root of the config file. Only `[mcp.limits]` is understood today; unknown
/// sections are rejected for the same reason unknown keys are.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ConfigFile {
    mcp: McpSection,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct McpSection {
    limits: ExtractionLimits,
}

/// Config file locations searched when `--config` is not given, in order.
const CWD_CONFIG: &str = "telemaco.toml";
const HOME_CONFIG: &str = ".config/telemaco/config.toml";

/// Every environment override, paired with the field it sets.
const ENV_KEYS: &[(&str, fn(&mut ExtractionLimits, usize))] = &[
    ("TELEMACO_MCP_MAX_CHARS", |l, v| l.max_chars = v),
    ("TELEMACO_MCP_MAX_LINKS", |l, v| l.max_links = v),
    ("TELEMACO_MCP_MAX_INTERACTIVE", |l, v| l.max_interactive = v),
    ("TELEMACO_MCP_MAX_SEARCH_RESULTS", |l, v| l.max_search_results = v),
    ("TELEMACO_MCP_SEARCH_CONTEXT_CHARS", |l, v| l.search_context_chars = v),
    ("TELEMACO_MCP_MAX_NETWORK_REQUESTS", |l, v| l.max_network_requests = v),
    ("TELEMACO_MCP_MAX_CONSOLE_MESSAGES", |l, v| l.max_console_messages = v),
    ("TELEMACO_MCP_MAX_FORMS", |l, v| l.max_forms = v),
];

/// Apply environment overrides on top of `base`.
///
/// The lookup is injected rather than read from the process environment so the
/// precedence rules can be unit-tested without mutating global state, following
/// `navigation_timeout_from_env_value` in `telemaco-browser`.
///
/// An unparsable value is ignored and leaves the lower layer in place: a typo in
/// a shell export must not silently clamp output to zero.
pub fn apply_env_overrides<F>(base: ExtractionLimits, lookup: F) -> ExtractionLimits
where
    F: Fn(&str) -> Option<String>,
{
    let mut limits = base;
    for (key, set) in ENV_KEYS {
        if let Some(value) = lookup(key).and_then(|v| v.trim().parse::<usize>().ok()) {
            set(&mut limits, value);
        }
    }
    limits
}

/// Parse config file contents. Separated from IO so malformed-input behavior is
/// directly testable.
pub fn parse_config(contents: &str) -> Result<ExtractionLimits> {
    let parsed: ConfigFile = toml::from_str(contents)?;
    Ok(parsed.mcp.limits)
}

/// The config file to read, or `None` for defaults.
///
/// An explicit `--config` path that does not exist is an error: the operator
/// named a file, so silently ignoring it would hide the mistake. A discovered
/// path that does not exist simply means "no config".
fn resolve_path(explicit: Option<&Path>) -> Result<Option<PathBuf>> {
    if let Some(path) = explicit {
        if !path.is_file() {
            anyhow::bail!("config file not found: {}", path.display());
        }
        return Ok(Some(path.to_path_buf()));
    }

    let cwd = PathBuf::from(CWD_CONFIG);
    if cwd.is_file() {
        return Ok(Some(cwd));
    }

    if let Some(home) = std::env::var_os("HOME") {
        let home_path = PathBuf::from(home).join(HOME_CONFIG);
        if home_path.is_file() {
            return Ok(Some(home_path));
        }
    }

    Ok(None)
}

/// Resolve the limits for this process: defaults, then the config file, then
/// the environment. CLI flags are layered on by the caller, and tool arguments
/// per call, so this is everything below those two.
///
/// A malformed or unreadable config is a hard error at startup rather than a
/// fallback to defaults: a broken config that silently does nothing is worse
/// than a server that refuses to start.
pub fn load(explicit: Option<&Path>) -> Result<ExtractionLimits> {
    let from_file = match resolve_path(explicit)? {
        Some(path) => {
            let contents = std::fs::read_to_string(&path)
                .with_context(|| format!("reading config file {}", path.display()))?;
            parse_config(&contents)
                .with_context(|| format!("parsing config file {}", path.display()))?
        }
        None => ExtractionLimits::default(),
    };

    Ok(apply_env_overrides(from_file, |key| std::env::var(key).ok()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn defaults_match_the_documented_values() {
        let limits = ExtractionLimits::default();
        assert_eq!(limits.max_chars, 4_000);
        assert_eq!(limits.max_links, 100);
        assert_eq!(limits.max_interactive, 100);
        assert_eq!(limits.max_search_results, 10);
        assert_eq!(limits.search_context_chars, 80);
    }

    #[test]
    fn empty_config_yields_defaults() {
        assert_eq!(parse_config("").unwrap(), ExtractionLimits::default());
    }

    #[test]
    fn partial_config_leaves_other_fields_at_default() {
        let limits = parse_config("[mcp.limits]\nmax_chars = 20000\n").unwrap();
        assert_eq!(limits.max_chars, 20_000);
        assert_eq!(limits.max_links, DEFAULT_MAX_LINKS);
    }

    #[test]
    fn unknown_key_is_rejected() {
        let err = parse_config("[mcp.limits]\nmax_charz = 20000\n").unwrap_err();
        assert!(
            err.to_string().contains("max_charz"),
            "error should name the offending key, got: {err}"
        );
    }

    #[test]
    fn unknown_section_is_rejected() {
        assert!(parse_config("[nope]\nx = 1\n").is_err());
    }

    #[test]
    fn malformed_toml_is_rejected() {
        assert!(parse_config("[mcp.limits\nmax_chars = ").is_err());
    }

    #[test]
    fn env_overrides_the_file() {
        let from_file = parse_config("[mcp.limits]\nmax_chars = 20000\n").unwrap();
        let resolved = apply_env_overrides(from_file, |key| {
            (key == "TELEMACO_MCP_MAX_CHARS").then(|| "50000".to_string())
        });
        assert_eq!(resolved.max_chars, 50_000);
    }

    #[test]
    fn env_leaves_untouched_fields_alone() {
        let resolved = apply_env_overrides(ExtractionLimits::default(), |key| {
            (key == "TELEMACO_MCP_MAX_LINKS").then(|| "7".to_string())
        });
        assert_eq!(resolved.max_links, 7);
        assert_eq!(resolved.max_chars, DEFAULT_MAX_CHARS);
    }

    #[test]
    fn unparsable_env_value_keeps_the_lower_layer() {
        let from_file = parse_config("[mcp.limits]\nmax_chars = 20000\n").unwrap();
        let resolved = apply_env_overrides(from_file, |key| {
            (key == "TELEMACO_MCP_MAX_CHARS").then(|| "banana".to_string())
        });
        assert_eq!(resolved.max_chars, 20_000);
    }

    #[test]
    fn every_env_key_is_wired() {
        for (key, _) in ENV_KEYS {
            let resolved =
                apply_env_overrides(ExtractionLimits::default(), |k| (k == *key).then(|| "3".to_string()));
            assert_ne!(
                resolved,
                ExtractionLimits::default(),
                "{key} parsed but changed nothing"
            );
        }
    }

    #[test]
    fn zero_means_unlimited() {
        assert_eq!(ExtractionLimits::cap(0), usize::MAX);
        assert_eq!(ExtractionLimits::cap(10), 10);
    }

    #[test]
    fn tool_argument_wins_over_config() {
        assert_eq!(ExtractionLimits::override_with(4_000, Some(10)), 10);
        assert_eq!(ExtractionLimits::override_with(4_000, None), 4_000);
        assert_eq!(ExtractionLimits::override_with(4_000, Some(0)), usize::MAX);
    }

    #[test]
    fn missing_explicit_path_is_an_error() {
        let err = resolve_path(Some(Path::new("/nonexistent/telemaco.toml"))).unwrap_err();
        assert!(err.to_string().contains("config file not found"));
    }

    /// The shipped example must stay in step with the code. Without this, the
    /// file people copy drifts from the defaults it claims to document.
    #[test]
    fn shipped_example_parses_and_states_the_real_defaults() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../telemaco.example.toml");
        let contents = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("reading {path}: {e}"));
        let parsed = parse_config(&contents).expect("example config must parse");
        assert_eq!(
            parsed,
            ExtractionLimits::default(),
            "telemaco.example.toml has drifted from the built-in defaults"
        );
    }

    #[test]
    fn env_only_config_needs_no_file() {
        let resolved = apply_env_overrides(ExtractionLimits::default(), no_env);
        assert_eq!(resolved, ExtractionLimits::default());
    }
}
