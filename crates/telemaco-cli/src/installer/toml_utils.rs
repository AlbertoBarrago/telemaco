use crate::installer::instructions::Action;

fn find_header_index(content: &str, header: &str) -> Option<usize> {
    let header_line = format!("[{}]", header);
    let mut current_idx = 0;
    for line in content.lines() {
        if line.trim() == header_line {
            return Some(current_idx);
        }
        current_idx += line.len() + 1; // +1 for '\n'
    }
    None
}

fn find_next_table_header(content: &str, start_idx: usize) -> usize {
    let mut current_idx = start_idx;
    let slice = &content[start_idx..];
    for line in slice.lines() {
        let trimmed = line.trim();
        // A new section header starts with '[' at the start of line (not inside a multiline string/array)
        if trimmed.starts_with('[') && trimmed.ends_with(']') && current_idx > start_idx {
            return current_idx;
        }
        current_idx += line.len() + 1;
    }
    content.len()
}

pub fn remove_toml_table(file_content: &str, header: &str) -> (String, Action) {
    let header_idx = match find_header_index(file_content, header) {
        Some(idx) => idx,
        None => return (file_content.to_string(), Action::NotFound),
    };

    let header_line = format!("[{}]", header);
    let block_end = find_next_table_header(file_content, header_idx + header_line.len());

    let before = file_content[..header_idx].trim_end();
    let after = file_content[block_end..].trim_start();

    let joined = if before.is_empty() {
        after.to_string()
    } else if after.is_empty() {
        before.to_string()
    } else {
        format!("{}\n\n{}", before, after)
    };

    let new_content = if joined.trim().is_empty() {
        String::new()
    } else {
        format!("{}\n", joined.trim())
    };

    (new_content, Action::Removed)
}

/// True for a line that opens a new TOML table, `[x]` or `[[x]]`.
fn is_table_header(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('[') && t.ends_with(']')
}

/// Byte offset of the first table header, i.e. the end of the top-level
/// preamble where bare keys live.
fn preamble_end(content: &str) -> usize {
    let mut idx = 0;
    for line in content.lines() {
        if is_table_header(line) {
            return idx;
        }
        idx += line.len() + 1;
    }
    content.len()
}

fn split_key_value(line: &str) -> Option<(&str, &str)> {
    let (k, v) = line.split_once('=')?;
    Some((k.trim(), v.trim()))
}

/// Removes a bare top-level key only when it holds exactly `value`.
///
/// Scoping to the preamble matters: a blanket `content.replace(...)` also hit
/// the same key inside the user's `[profiles.*]` tables.
///
/// Used to clean up the `hooks = true` older versions wrote at the top level:
/// in Codex `hooks` is the table that holds inline hook definitions, so a
/// blind removal would take the user's own hooks with it.
pub fn remove_top_level_key_with_value(content: &str, key: &str, value: &str) -> (String, bool) {
    remove_top_level_key_matching(content, key, |v| v == value)
}

fn remove_top_level_key_matching(
    content: &str,
    key: &str,
    value_matches: impl Fn(&str) -> bool,
) -> (String, bool) {
    let split = preamble_end(content);
    let (preamble, rest) = content.split_at(split);

    let kept: Vec<&str> = preamble
        .lines()
        .filter(|l| !matches!(split_key_value(l), Some((k, v)) if k == key && value_matches(v)))
        .collect();
    if kept.len() == preamble.lines().count() {
        return (content.to_string(), false);
    }

    let mut new_content = String::new();
    for l in kept {
        new_content.push_str(l);
        new_content.push('\n');
    }
    new_content.push_str(rest);
    (new_content, true)
}

/// Upserts a table, rewriting only the keys we manage.
///
/// The previous version replaced the whole table, which silently dropped
/// anything the user had added there (`env`, `startup_timeout_ms`, comments).
pub fn upsert_toml_table_keys(
    file_content: &str,
    header: &str,
    managed: &[(&str, String)],
) -> (String, Action) {
    let header_line = format!("[{}]", header);

    let Some(header_idx) = find_header_index(file_content, header) else {
        let mut block = header_line;
        for (k, v) in managed {
            block.push('\n');
            block.push_str(&format!("{} = {}", k, v));
        }
        let trimmed = file_content.trim_end();
        let sep = if trimmed.is_empty() { "" } else { "\n\n" };
        return (format!("{}{}{}\n", trimmed, sep, block), Action::Created);
    };

    let block_end = find_next_table_header(file_content, header_idx + header_line.len());
    let existing = &file_content[header_idx..block_end];

    let mut lines: Vec<String> = existing.lines().map(|l| l.to_string()).collect();
    let mut changed = false;

    for (key, value) in managed {
        let desired = format!("{} = {}", key, value);
        match lines
            .iter_mut()
            .find(|l| matches!(split_key_value(l), Some((k, _)) if k == *key))
        {
            Some(line) => {
                if *line != desired {
                    *line = desired;
                    changed = true;
                }
            }
            None => {
                // Keep user keys in place; append ours after the last one.
                let insert_at = lines
                    .iter()
                    .rposition(|l| !l.trim().is_empty())
                    .map_or(lines.len(), |i| i + 1);
                lines.insert(insert_at, desired);
                changed = true;
            }
        }
    }

    if !changed {
        return (file_content.to_string(), Action::Unchanged);
    }

    let rebuilt = lines.join("\n");
    let before = &file_content[..header_idx];
    let after = &file_content[block_end..];
    let sep = if existing.ends_with('\n') { "\n" } else { "" };
    (
        format!("{}{}{}{}", before, rebuilt, sep, after),
        Action::Updated,
    )
}

/// The value a table holds for `key`, verbatim, or `None` when either the
/// table or the key is missing.
///
/// Reading before writing is the point: `[features] hooks` is a switch the
/// user owns, and the installer needs to know what they set it to rather than
/// overwrite it.
pub fn toml_table_key_value(content: &str, header: &str, key: &str) -> Option<String> {
    let header_idx = find_header_index(content, header)?;
    let header_line = format!("[{}]", header);
    let block_end = find_next_table_header(content, header_idx + header_line.len());
    content[header_idx..block_end]
        .lines()
        .skip(1)
        .find_map(|l| match split_key_value(l) {
            Some((k, v)) if k == key => Some(v.to_string()),
            _ => None,
        })
}

/// Removes a single key from a table, leaving the user's other keys in place.
///
/// The table goes with it when nothing but the header is left, so uninstalling
/// does not leave an empty `[features]` behind; a table that still holds
/// comments is kept.
pub fn remove_toml_table_key(content: &str, header: &str, key: &str) -> (String, bool) {
    let Some(header_idx) = find_header_index(content, header) else {
        return (content.to_string(), false);
    };
    let header_line = format!("[{}]", header);
    let block_end = find_next_table_header(content, header_idx + header_line.len());
    let block = &content[header_idx..block_end];

    let kept: Vec<&str> = block
        .lines()
        .filter(|l| !matches!(split_key_value(l), Some((k, _)) if k == key))
        .collect();
    if kept.len() == block.lines().count() {
        return (content.to_string(), false);
    }

    let has_content = kept
        .iter()
        .skip(1)
        .any(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'));
    if !has_content {
        let (out, _) = remove_toml_table(content, header);
        return (out, true);
    }

    let mut rebuilt = kept.join("\n");
    if block.ends_with('\n') {
        rebuilt.push('\n');
    }
    (
        format!("{}{}{}", &content[..header_idx], rebuilt, &content[block_end..]),
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_upsert_table_creates_and_updates_managed_keys() {
        let input = "model = \"test\"\n";
        let managed = [("command", "\"telemaco\"".to_string())];
        let (output, action) = upsert_toml_table_keys(input, "mcp_servers.telemaco", &managed);
        assert_eq!(action, Action::Created);
        assert!(output.contains("[mcp_servers.telemaco]"));
        assert!(output.contains("model = \"test\""));

        let managed = [("command", "\"new\"".to_string())];
        let (output2, action) = upsert_toml_table_keys(&output, "mcp_servers.telemaco", &managed);
        assert_eq!(action, Action::Updated);
        assert!(output2.contains("command = \"new\""));

        let (_, action) = upsert_toml_table_keys(&output2, "mcp_servers.telemaco", &managed);
        assert_eq!(action, Action::Unchanged);
    }

    #[test]
    fn test_upsert_table_keeps_user_keys() {
        let input = "[mcp_servers.telemaco]\ncommand = \"old\"\nstartup_timeout_ms = 30000\nenv = { FOO = \"bar\" }\n\n[other]\nk = 1\n";
        let managed = [
            ("command", "\"new\"".to_string()),
            ("args", "[\"mcp\"]".to_string()),
        ];
        let (output, action) = upsert_toml_table_keys(input, "mcp_servers.telemaco", &managed);
        assert_eq!(action, Action::Updated);
        assert!(output.contains("command = \"new\""));
        assert!(output.contains("args = [\"mcp\"]"));
        // Anything the user added to our table has to survive a reinstall.
        assert!(output.contains("startup_timeout_ms = 30000"), "{}", output);
        assert!(output.contains("env = { FOO = \"bar\" }"), "{}", output);
        assert!(output.contains("[other]"));
    }

    #[test]
    fn test_remove_table_key_keeps_the_user_keys() {
        let input = "[features]\nhooks = true\nweb_search = true\n\n[other]\nk = 1\n";
        let (output, removed) = remove_toml_table_key(input, "features", "hooks");
        assert!(removed);
        assert!(!output.contains("hooks = true"), "{}", output);
        assert!(output.contains("web_search = true"), "{}", output);
        assert!(output.contains("[other]"), "{}", output);

        // Nothing left under the header: the table goes too.
        let only = "[features]\nhooks = true\n\n[other]\nk = 1\n";
        let (output, removed) = remove_toml_table_key(only, "features", "hooks");
        assert!(removed);
        assert!(!output.contains("[features]"), "{}", output);
        assert!(output.contains("[other]"), "{}", output);

        let (_, removed) = remove_toml_table_key("[other]\nk = 1\n", "features", "hooks");
        assert!(!removed);
    }

    #[test]
    fn test_remove_top_level_key_only_when_the_value_matches() {
        let ours = "hooks = true\nmodel = \"x\"\n";
        let (output, removed) = remove_top_level_key_with_value(ours, "hooks", "true");
        assert!(removed);
        assert!(!output.contains("hooks"), "{}", output);

        // An inline hooks table the user wrote must survive.
        let theirs = "hooks = { UserPromptSubmit = [] }\nmodel = \"x\"\n";
        let (output, removed) = remove_top_level_key_with_value(theirs, "hooks", "true");
        assert!(!removed);
        assert_eq!(output, theirs);

        // The same key inside one of the user's tables is not ours to touch.
        let in_table = "[profiles.work]\nhooks = true\nmodel = \"x\"\n";
        let (output, removed) = remove_top_level_key_with_value(in_table, "hooks", "true");
        assert!(!removed);
        assert_eq!(output, in_table);
    }

    #[test]
    fn test_remove_toml() {
        let input = "model = \"test\"\n\n[mcp_servers.telemaco]\ncommand = \"new\"\n\n[other]\nkey = 1\n";
        let (output, action) = remove_toml_table(input, "mcp_servers.telemaco");
        assert_eq!(action, Action::Removed);
        assert!(!output.contains("[mcp_servers.telemaco]"));
        assert!(output.contains("[other]"));
    }
}
