use std::fs;
use std::path::{Path, PathBuf};
use serde_json::Value;
use crate::installer::text_utils::with_line_ending;

/// Appended to a config file before we rewrite it in a way that loses
/// formatting, so the original stays recoverable. Same suffix the shell
/// installer used.
const BACKUP_SUFFIX: &str = ".telemaco-backup";

/// The file a write should actually land on.
///
/// Config files are routinely symlinks into a dotfiles repo (stow, chezmoi,
/// yadm). Renaming over the link would replace it with a regular file and
/// orphan the copy the user actually edits, so the link is followed first.
fn resolve_write_target(file_path: &Path) -> PathBuf {
    match fs::symlink_metadata(file_path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            fs::canonicalize(file_path).unwrap_or_else(|_| file_path.to_path_buf())
        }
        _ => file_path.to_path_buf(),
    }
}

pub fn atomic_write_file(file_path: &Path, content: &str) -> std::io::Result<()> {
    let target = resolve_write_target(file_path);

    if let Some(parent) = target.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    let pid = std::process::id();
    let tmp_path = target.with_extension(format!("tmp.{}", pid));
    fs::write(&tmp_path, content)?;

    // A fresh temp file is created with the process umask. Carry the original
    // mode over instead: these configs hold credentials and are often 0600.
    if let Ok(meta) = fs::metadata(&target) {
        let _ = fs::set_permissions(&tmp_path, meta.permissions());
    }

    if let Err(e) = fs::rename(&tmp_path, &target) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }
    Ok(())
}

/// How a JSON config file parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonSource {
    /// Absent or empty: the caller starts from an empty object.
    Missing,
    /// Strict JSON. Rewriting it preserves everything but the formatting.
    Strict,
    /// Parsed only after stripping comments and trailing commas, which VS Code,
    /// Cursor and Zed all accept in their config files. Rewriting it as strict
    /// JSON drops those comments, so the file must be backed up first.
    Jsonc,
}

#[derive(Debug)]
pub struct JsonConfig {
    pub value: Value,
    pub source: JsonSource,
}

/// Reads a JSON config for inspection only. Never fails: detection has nothing
/// to lose from an unreadable file and never writes one back.
pub fn read_json_file(file_path: &Path) -> Value {
    match parse_json_file(file_path) {
        Ok(cfg) if cfg.value.is_object() => cfg.value,
        _ => empty_object(),
    }
}

/// Reads a JSON config that is about to be modified.
///
/// `Err` means the file exists but cannot be parsed, or does not hold an
/// object. The caller must then leave it alone: overwriting it would discard
/// the user's other MCP servers and editor settings.
pub fn read_json_for_update(file_path: &Path) -> Result<JsonConfig, String> {
    let cfg = parse_json_file(file_path)?;
    if !cfg.value.is_object() {
        return Err(format!(
            "{}: top level is not a JSON object; leaving it alone",
            file_path.display()
        ));
    }
    Ok(cfg)
}

/// Where `backup_file` puts its copy.
pub fn backup_path(file_path: &Path) -> PathBuf {
    let mut name = file_path.as_os_str().to_os_string();
    name.push(BACKUP_SUFFIX);
    PathBuf::from(name)
}

/// Copies a file next to itself before a rewrite that would lose content.
pub fn backup_file(file_path: &Path) -> Result<PathBuf, String> {
    let backup = backup_path(file_path);
    fs::copy(file_path, &backup)
        .map_err(|e| format!("Could not back up {}: {}", file_path.display(), e))?;
    Ok(backup)
}

pub fn write_json_file(file_path: &Path, data: &Value) -> Result<(), String> {
    let mut json_str = serde_json::to_string_pretty(data)
        .map_err(|e| format!("Could not serialize JSON for {}: {}", file_path.display(), e))?;
    json_str.push('\n');
    // A config the user keeps with CRLF stays that way. Serializing always
    // emits LF, which turns adding one server into a whole-file diff for
    // anyone on Windows or with core.autocrlf. Newlines inside JSON strings
    // are escaped, so only the structural ones are touched.
    if let Ok(original) = fs::read_to_string(file_path) {
        json_str = with_line_ending(&original, &json_str);
    }
    atomic_write_file(file_path, &json_str)
        .map_err(|e| format!("Could not write {}: {}", file_path.display(), e))
}

pub fn json_deep_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                return false;
            }
            a.iter().zip(b.iter()).all(|(x, y)| json_deep_equal(x, y))
        }
        (Value::Object(a), Value::Object(b)) => {
            if a.len() != b.len() {
                return false;
            }
            for (k, v_a) in a {
                match b.get(k) {
                    Some(v_b) => {
                        if !json_deep_equal(v_a, v_b) {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
            true
        }
        _ => false,
    }
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}

fn parse_json_file(file_path: &Path) -> Result<JsonConfig, String> {
    if !file_path.exists() {
        return Ok(JsonConfig { value: empty_object(), source: JsonSource::Missing });
    }

    let text = fs::read_to_string(file_path)
        .map_err(|e| format!("Could not read {}: {}", file_path.display(), e))?;
    // Editors on Windows leave a UTF-8 BOM in front; serde_json rejects it and
    // the file would be refused as malformed.
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text).to_string();
    if text.trim().is_empty() {
        return Ok(JsonConfig { value: empty_object(), source: JsonSource::Missing });
    }

    match serde_json::from_str(&text) {
        Ok(value) => Ok(JsonConfig { value, source: JsonSource::Strict }),
        Err(strict_err) => match serde_json::from_str(&strip_jsonc(&text)) {
            Ok(value) => Ok(JsonConfig { value, source: JsonSource::Jsonc }),
            Err(_) => Err(format!(
                "{} is not valid JSON ({}); leaving it alone",
                file_path.display(),
                strict_err
            )),
        },
    }
}

/// Strips `//` and `/* */` comments and trailing commas, leaving string
/// literals untouched. Newlines inside comments are kept so parse errors still
/// point at the right line.
fn strip_jsonc(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;

    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }

        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' => match chars.peek().copied() {
                Some('/') => {
                    for c in chars.by_ref() {
                        if c == '\n' {
                            out.push('\n');
                            break;
                        }
                    }
                }
                Some('*') => {
                    chars.next();
                    let mut prev = '\0';
                    for c in chars.by_ref() {
                        if prev == '*' && c == '/' {
                            break;
                        }
                        if c == '\n' {
                            out.push('\n');
                        }
                        prev = c;
                    }
                }
                _ => out.push(c),
            },
            _ => out.push(c),
        }
    }

    strip_trailing_commas(&out)
}

fn strip_trailing_commas(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_string = false;
    let mut escaped = false;
    // Byte offset in `out` of the last comma that is still only followed by
    // whitespace; dropped if the next real character closes the container.
    let mut pending_comma: Option<usize> = None;

    for c in input.chars() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }

        match c {
            '"' => {
                pending_comma = None;
                in_string = true;
                out.push(c);
            }
            ',' => {
                pending_comma = Some(out.len());
                out.push(c);
            }
            ']' | '}' => {
                if let Some(idx) = pending_comma.take() {
                    out.remove(idx);
                }
                out.push(c);
            }
            _ if c.is_whitespace() => out.push(c),
            _ => {
                pending_comma = None;
                out.push(c);
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn temp_path(name: &str) -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("telemaco_json_{}_{}_{}", std::process::id(), id, name))
    }

    #[test]
    fn test_strip_line_and_block_comments() {
        let src = "{\n  // a comment\n  \"a\": 1, /* inline */\n  \"b\": 2\n}";
        let value: Value = serde_json::from_str(&strip_jsonc(src)).unwrap();
        assert_eq!(value["a"], 1);
        assert_eq!(value["b"], 2);
    }

    #[test]
    fn test_strip_leaves_comment_markers_inside_strings() {
        let src = r#"{"url": "https://example.com/x", "path": "a/*b*/c"}"#;
        let value: Value = serde_json::from_str(&strip_jsonc(src)).unwrap();
        assert_eq!(value["url"], "https://example.com/x");
        assert_eq!(value["path"], "a/*b*/c");
    }

    #[test]
    fn test_strip_trailing_commas() {
        let src = "{\n  \"a\": [1, 2, 3,],\n  \"b\": {\"c\": 1,},\n}";
        let value: Value = serde_json::from_str(&strip_jsonc(src)).unwrap();
        assert_eq!(value["a"], serde_json::json!([1, 2, 3]));
        assert_eq!(value["b"]["c"], 1);
    }

    #[test]
    fn test_read_for_update_reports_jsonc() {
        let path = temp_path("jsonc");
        fs::write(&path, "{\n  // keep me\n  \"mcpServers\": {\"pg\": {}}\n}").unwrap();
        let cfg = read_json_for_update(&path).unwrap();
        assert_eq!(cfg.source, JsonSource::Jsonc);
        assert!(cfg.value["mcpServers"]["pg"].is_object());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_read_for_update_accepts_a_bom() {
        let path = temp_path("bom.json");
        fs::write(&path, "\u{feff}{\"mcpServers\":{\"pg\":{}}}").unwrap();
        let cfg = read_json_for_update(&path).unwrap();
        assert_eq!(cfg.source, JsonSource::Strict);
        assert!(cfg.value["mcpServers"]["pg"].is_object());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_read_for_update_refuses_malformed() {
        let path = temp_path("broken");
        fs::write(&path, "{\"mcpServers\": ").unwrap();
        let err = read_json_for_update(&path).unwrap_err();
        assert!(err.contains("leaving it alone"), "{}", err);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_read_for_update_refuses_non_object() {
        let path = temp_path("array");
        fs::write(&path, "[1, 2, 3]").unwrap();
        let err = read_json_for_update(&path).unwrap_err();
        assert!(err.contains("not a JSON object"), "{}", err);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_write_preserves_mode_and_symlink() {
        let target = temp_path("mode.json");
        fs::write(&target, "{}").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
        }

        write_json_file(&target, &serde_json::json!({"a": 1})).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "a config that was 0600 must not become world-readable");
        }

        // Writing through a symlink has to keep the link and update its target.
        #[cfg(unix)]
        {
            let link = temp_path("link.json");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            write_json_file(&link, &serde_json::json!({"a": 2})).unwrap();
            assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
            let through: Value = serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
            assert_eq!(through["a"], 2);
            let _ = fs::remove_file(&link);
        }
        let _ = fs::remove_file(&target);
    }

    #[test]
    fn test_backup_appends_suffix_without_losing_extension() {
        let path = temp_path("cfg.json");
        fs::write(&path, "{}").unwrap();
        let backup = backup_file(&path).unwrap();
        assert!(backup.to_string_lossy().ends_with("cfg.json.telemaco-backup"));
        assert!(backup.exists());
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&backup);
    }
}
