use std::io::{self, Read, Seek, SeekFrom};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct HookPayload {
    // Claude & Codex format
    prompt: Option<String>,
    #[allow(dead_code)]
    cwd: Option<String>,

    // Antigravity format
    conversation_id: Option<String>,
    transcript_path: Option<String>,

    /// The event that fired, as the agent names it. Claude Code, Gemini CLI
    /// and Qwen Code all send it in snake_case; it is echoed back in
    /// `hookSpecificOutput` so the answer names the event that asked.
    #[serde(rename = "hook_event_name", alias = "hookEventName")]
    hook_event_name: Option<String>,

    /// Event-specific fields. Hermes Agent puts the submitted message in
    /// `extra.user_message` and leaves `tool_name`/`tool_input` null for
    /// `pre_llm_call` (hermes-agent.nousresearch.com/docs/user-guide/features/hooks).
    #[serde(default)]
    extra: Option<serde_json::Value>,
}

/// One step injected into Antigravity's trajectory.
///
/// The field has to be `ephemeralMessage`: an injected step carries a
/// `toolCall`, a `userMessage`, or an `ephemeralMessage` ("a transient system
/// message"), and nothing else (antigravity.google/docs/hooks). The
/// `systemMessage` we used to send is not part of the schema, so the directive
/// was dropped on the floor.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AntigravityInjectedStep {
    ephemeral_message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AntigravityPreInvocationResult {
    inject_steps: Vec<AntigravityInjectedStep>,
}

pub const TELEMACO_DIRECTIVE: &str = r#"<telemaco_directive>
MANDATORY WEB & SEARCH DIRECTIVE:
Whenever visiting URLs, searching documentation, or inspecting web content for this request, YOU MUST USE TELEMACO tools (browser_navigate, browser_markdown, browser_snapshot) or `telemaco fetch <url> --dump markdown` instead of built-in search or curl.
- To search the web: browser_navigate(url="https://duckduckgo.com/html/?q=<query>") then browser_markdown()
- Open page: browser_navigate(url=...)
- Read content as clean Markdown: browser_markdown()
- Inspect DOM / form controls: browser_snapshot()
- Source transparency: Always state the target URL being visited (e.g. `Navigating to: <url>`) and cite source URLs in your answer.
Do NOT use generic search summaries when Telemaco can inspect the real rendered page.
</telemaco_directive>"#;

/// How an agent wants the hook's answer on stdout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Plain text, added to the model's context as-is. Claude Code, Codex,
    /// Factory Droid and Kiro all read stdout this way.
    Text,
    /// A JSON object carrying `hookSpecificOutput.additionalContext`. Qwen Code
    /// parses stdout as JSON and ignores anything else, so plain text there was
    /// thrown away (QwenLM/qwen-code docs/users/features/hooks.md).
    Json,
    /// A JSON object carrying `context`, which is what Hermes Agent's
    /// `pre_llm_call` shell hook injects into the next LLM turn
    /// (hermes-agent.nousresearch.com/docs/user-guide/features/hooks).
    Hermes,
    /// A JSON object carrying `hook_specific_output.additional_context`, the
    /// snake_case decision object Poolside parses on `UserPromptSubmit`.
    /// "Use the decision field names exactly as shown. `pool` ignores fields it
    /// does not recognize, so well-formed JSON with a misspelled field can
    /// produce no error and have no effect" (docs.poolside.ai/hooks).
    Poolside,
    /// A JSON object carrying `additional_context`, which is what Cursor's
    /// `sessionStart` hook adds to "the conversation's initial system context"
    /// (cursor.com/docs/hooks). That event carries no prompt, so this format
    /// answers every session rather than only the ones that look web-bound.
    Cursor,
}

impl OutputFormat {
    pub fn parse(s: &str) -> Option<OutputFormat> {
        match s {
            "text" => Some(OutputFormat::Text),
            "json" => Some(OutputFormat::Json),
            "cursor" => Some(OutputFormat::Cursor),
            "hermes" => Some(OutputFormat::Hermes),
            "poolside" => Some(OutputFormat::Poolside),
            _ => None,
        }
    }

    /// `event` is the name the agent used on stdin. Gemini CLI injects context
    /// from `BeforeAgent`, not `UserPromptSubmit`, so a hardcoded event name
    /// answered a question nobody had asked
    /// (geminicli.com/docs/hooks/reference, qwen-code docs/features/hooks).
    fn render(self, directive: &str, event: &str) -> String {
        match self {
            OutputFormat::Text => directive.to_string(),
            OutputFormat::Cursor => {
                serde_json::json!({ "additional_context": directive }).to_string()
            }
            OutputFormat::Hermes => serde_json::json!({ "context": directive }).to_string(),
            OutputFormat::Poolside => serde_json::json!({
                "hook_specific_output": { "additional_context": directive }
            })
            .to_string(),
            OutputFormat::Json => serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": event,
                    "additionalContext": directive,
                }
            })
            .to_string(),
        }
    }
}

/// The stdin-only case, which is every agent but Kiro.
#[cfg(test)]
pub fn process_hook_input_as(raw: &str, format: OutputFormat) -> Option<String> {
    process_hook_input_with(raw, format, None)
}

/// `ambient_prompt` is the prompt an agent passes in the environment rather
/// than on stdin. Kiro is the one that does it: "When using the shell command
/// action, the user prompt can be accessed via the `USER_PROMPT` environment
/// variable" (kiro.dev/docs/hooks/types, Prompt Submit). Without it, the
/// stdin payload carries session context and no prompt, so nothing ever looked
/// web-bound and the hook stayed silent.
pub fn process_hook_input_with(
    raw: &str,
    format: OutputFormat,
    ambient_prompt: Option<&str>,
) -> Option<String> {
    let ambient_prompt = ambient_prompt.filter(|p| !p.trim().is_empty());
    if raw.trim().is_empty() && ambient_prompt.is_none() {
        return None;
    }

    // Cursor's `sessionStart` fires once, before any prompt exists, so there is
    // nothing to look for web intent in: the directive goes in every session,
    // the way an always-applied rule does for a project.
    if format == OutputFormat::Cursor {
        return Some(format.render(TELEMACO_DIRECTIVE, "sessionStart"));
    }

    let payload: HookPayload = if raw.trim().is_empty() {
        HookPayload::default()
    } else {
        serde_json::from_str(raw).ok()?
    };
    let is_antigravity = payload.conversation_id.is_some() || payload.transcript_path.is_some();

    let extra_message = payload
        .extra
        .as_ref()
        .and_then(|e| e.get("user_message"))
        .and_then(|m| m.as_str())
        .map(|m| m.to_string());

    let prompt = if let Some(p) = payload.prompt.filter(|p| !p.trim().is_empty()) {
        Some(p)
    } else if let Some(p) = extra_message.filter(|p| !p.trim().is_empty()) {
        Some(p)
    } else if let Some(p) = ambient_prompt {
        Some(p.to_string())
    } else if let Some(ref path) = payload.transcript_path {
        extract_prompt_from_transcript(path)
    } else {
        None
    };

    let has_intent = prompt.as_deref().map_or(false, has_web_intent);

    if is_antigravity {
        if has_intent {
            let res = AntigravityPreInvocationResult {
                inject_steps: vec![AntigravityInjectedStep {
                    ephemeral_message: TELEMACO_DIRECTIVE.to_string(),
                }],
            };
            serde_json::to_string(&res).ok()
        } else {
            Some("{}".to_string())
        }
    } else if has_intent {
        let event = payload.hook_event_name.as_deref().unwrap_or("UserPromptSubmit");
        Some(format.render(TELEMACO_DIRECTIVE, event))
    } else {
        None
    }
}

pub fn run_prompt_hook(format: OutputFormat) -> anyhow::Result<()> {
    if std::env::var("TELEMACO_NO_PROMPT_HOOK").as_deref() == Ok("1")
        || std::env::var("TELEMACO_PROMPT_HOOK").as_deref() == Ok("0")
    {
        return Ok(());
    }

    use std::io::IsTerminal;
    if io::stdin().is_terminal() {
        return Ok(());
    }

    let mut raw = String::new();
    let read = io::stdin().read_to_string(&mut raw).is_ok();
    let ambient = std::env::var("USER_PROMPT").ok();
    let has_ambient = ambient.as_deref().map_or(false, |p| !p.trim().is_empty());
    // An agent that passes the prompt in the environment may send nothing at
    // all on stdin.
    if (!read || raw.trim().is_empty()) && !has_ambient {
        return Ok(());
    }

    if let Some(output) = process_hook_input_with(&raw, format, ambient.as_deref()) {
        println!("{}", output);
    }

    Ok(())
}

/// Last user message in a JSONL transcript, read from the tail of the file.
fn extract_prompt_from_transcript(path: &str) -> Option<String> {
    use std::fs::File;

    const TAIL_BYTES: u64 = 64 * 1024;

    let mut file = File::open(path).ok()?;
    let file_len = file.metadata().ok()?.len();
    let read_start = file_len.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(read_start)).ok()?;

    // Read bytes, not a String: seeking to a fixed offset lands mid-character
    // on any transcript with multibyte text, and `read_to_string` would fail
    // outright and lose a prompt that is sitting right there.
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    let chunk = String::from_utf8_lossy(&buf);

    for line in chunk.lines().rev() {
        // Parse the line instead of matching its exact bytes: the old check
        // wanted `"type":"USER_INPUT"` and missed any transcript written with
        // spaces after the colons.
        if !line.contains("USER_INPUT") {
            continue;
        }
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if val.get("type").and_then(|t| t.as_str()) != Some("USER_INPUT") {
            continue;
        }
        if let Some(content) = val.get("content").and_then(|c| c.as_str()) {
            return Some(content.to_string());
        }
    }
    None
}

/// Hostname suffixes that mark a token as a web address. Deliberately excludes
/// source-file extensions (`.rs`, `.js`, `.py`, ...) so ordinary code talk does
/// not read as a URL, and `.io`, which reads as an invented extension often
/// enough ("main.io") that the bare form is not worth the false positives. A
/// real .io address still matches through its scheme or `www.` prefix.
const TLDS: &[&str] = &[
    "com", "org", "net", "dev", "info", "edu", "gov", "ai",
    "it", "es", "fr", "de", "pt", "br", "uk",
];

/// Terms that on their own mean the user is talking about the live web.
/// Matched on word boundaries: "web" must not fire on "webpack", and "link"
/// must not fire on "linker".
const WEB_MARKERS: &[&str] = &[
    // Common / English
    "internet", "web", "online", "website", "webpage", "web page", "site",
    "url", "urls", "link", "links", "scrape", "scraping", "crawl",
    "google", "duckduckgo", "wikipedia",
    "stackoverflow",
    // Italian
    "sito", "siti", "naviga", "navigare", "in rete",
    // Spanish
    "sitio", "navega", "navegar", "en linea", "en línea", "en la red", "la red",
    // French
    "naviguer", "en ligne", "le net",
    // Portuguese
    "em linha", "na rede",
    // German
    "webseite", "im netz", "surfen",
];

/// Verb stems matched against the start of a word. Only stems long enough that
/// a prefix cannot land on an unrelated word live here: "cherch" covers
/// "cherche" and "chercher" without catching anything else.
const SEARCH_VERB_STEMS: &[&str] = &[
    // English. "brows" also covers "browser", which on its own is ordinary
    // code talk in a browser-engine repo and must not fire without a topic.
    "search", "find", "lookup", "consult", "visit", "brows",
    // Italian
    "cerc", "trov",
    // Spanish
    "busc", "encuentr",
    // French
    "cherch", "trouv", "recherch",
    // Portuguese
    "pesquis", "procur",
    // German
    "nachschlag",
];

/// Reading verbs matched as whole words instead.
///
/// Prefix-matching stems this short fired on everyday code vocabulary: "lis"
/// caught "list" and "listener", "ler" caught "lerp", "lee" caught "leetcode".
/// German "such" is left out entirely because English "such" is far more common
/// in a prompt than the German imperative, and "suche"/"suchen" still match.
const SEARCH_VERB_WORDS: &[&str] = &[
    // Spanish
    "lee", "leer",
    // French
    "lis", "lire", "lisez",
    // Portuguese
    "leia", "ler", "leio",
    // German
    "lies", "lest", "lesen", "suche", "suchen", "gesucht",
];

/// Reference material a search verb has to be pointed at before the prompt
/// counts as web intent. "search the docs" qualifies; "find the bug" does not.
const TOPICS: &[&str] = &[
    // English
    "doc", "docs", "documentation", "guide", "manual", "tutorial", "library",
    "spec", "specification", "repo", "repository", "package", "example",
    "examples", "api", "changelog", "release notes", "blog", "article",
    "github", "npm", "crate", "mdn",
    // Italian
    "guida", "manuale", "documentazione", "libreria", "pacchetto", "specifica",
    "esempio", "esempi",
    // Spanish
    "guía", "guia", "librería", "libreria", "biblioteca", "paquete", "ejemplo",
    "documentación", "documentacion",
    // French
    "manuel", "tutoriel", "documentation", "librairie", "bibliothèque",
    "bibliotheque", "paquet", "exemple",
    // Portuguese
    "biblioteca", "pacote", "exemplo", "documentação", "documentacao",
    // German
    "anleitung", "handbuch", "dokumentation", "bibliothek", "paket", "beispiel",
];

/// Substring search that only accepts whole words, so markers do not fire on
/// the middle of an identifier. Works for multi-word markers too.
fn contains_word(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let mut from = 0;
    while let Some(offset) = haystack[from..].find(needle) {
        let start = from + offset;
        let end = start + needle.len();
        let before_ok = haystack[..start]
            .chars()
            .next_back()
            .map_or(true, |c| !c.is_alphanumeric());
        let after_ok = haystack[end..]
            .chars()
            .next()
            .map_or(true, |c| !c.is_alphanumeric());
        if before_ok && after_ok {
            return true;
        }
        from = start + haystack[start..].chars().next().map_or(1, |c| c.len_utf8());
    }
    false
}

/// True when a whitespace-separated token is shaped like a hostname.
///
/// The point is to accept "example.com/docs" while rejecting the dotted tokens
/// that fill ordinary code prompts: ".iter()" has an empty first label and
/// "tree.rs" ends in an extension that is not a TLD.
fn looks_like_host(token: &str) -> bool {
    let trimmed = token.trim_matches(|c: char| {
        !c.is_alphanumeric() && c != '.' && c != '-' && c != '/' && c != ':'
    });
    let host = trimmed.split(['/', '?', '#', ':']).next().unwrap_or("");
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    if labels.iter().any(|l| l.is_empty()) {
        return false;
    }
    if !labels
        .iter()
        .all(|l| l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
    {
        return false;
    }
    TLDS.contains(&labels[labels.len() - 1])
}

fn has_web_intent(prompt: &str) -> bool {
    let lower = prompt.to_lowercase();

    // 1. Explicit URLs.
    if lower.contains("http://") || lower.contains("https://") || lower.contains("www.") {
        return true;
    }

    // 2. Tokens shaped like a hostname.
    if lower.split_whitespace().any(looks_like_host) {
        return true;
    }

    // 3. Terms that mean the live web on their own.
    if WEB_MARKERS.iter().any(|m| contains_word(&lower, m)) {
        return true;
    }

    // 4. A search verb pointed at external reference material.
    let has_search_verb = lower.split(|c: char| !c.is_alphanumeric()).any(|word| {
        !word.is_empty()
            && (SEARCH_VERB_STEMS.iter().any(|stem| word.starts_with(stem))
                || SEARCH_VERB_WORDS.contains(&word))
    });
    has_search_verb && TOPICS.iter().any(|t| contains_word(&lower, t))
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn test_short_verb_stems_do_not_fire_on_code_words() {
        // Every one of these carries a topic word, so the only thing keeping
        // them out is the verb list. Prefix-matching "lis", "ler", "lee" and
        // "such" used to turn each of them into a web prompt.
        for p in [
            "list the api endpoints in this repo",
            "add a listener for the example event",
            "use a lerp helper in the graphics library",
            "such an api example should be in the docs",
            "leetcode style examples for this api",
        ] {
            assert!(!has_web_intent(p), "false positive: {}", p);
        }
        // The reading verbs those stems were there for still match.
        assert!(has_web_intent("lee la documentación de react"));
        assert!(has_web_intent("lis la documentation de rust"));
        assert!(has_web_intent("suche die anleitung zu python"));
    }
    #[test]
    fn test_has_web_intent_urls() {
        assert!(has_web_intent("Check https://example.com/docs"));
        assert!(has_web_intent("Vai su www.rust-lang.org/learn"));
        assert!(has_web_intent("Guarda http://localhost:8080"));
        assert!(has_web_intent("Consulta el dominio ejemplo.es"));
    }

    #[test]
    fn test_has_web_intent_multilingual() {
        // English
        assert!(has_web_intent("search the web for the new Next.js docs"));
        assert!(has_web_intent("scrape the product table from this site"));

        // Italian
        assert!(has_web_intent("cerca online la documentazione ufficiale"));
        assert!(has_web_intent("trova la guida di github"));

        // Spanish
        assert!(has_web_intent("busca en la red el manual de react"));
        assert!(has_web_intent("encuentra la guía de github para ordersummary"));

        // French
        assert!(has_web_intent("cherche sur le net le tutoriel de rust"));
        assert!(has_web_intent("trouve la documentation de github"));

        // Portuguese
        assert!(has_web_intent("pesquisa na internet como criar um ordersummary"));
        assert!(has_web_intent("procura o tutorial de python"));

        // German
        assert!(has_web_intent("suche im web nach der anleitung für react"));
        assert!(has_web_intent("finde das handbuch zu python"));
    }

    #[test]
    fn test_has_web_intent_negative() {
        assert!(!has_web_intent("refactor this function to be more idiomatic"));
        assert!(!has_web_intent("fix the compilation error in main.rs"));
        assert!(!has_web_intent("scrivi una funzione per calcolare il fibonacci"));
        assert!(!has_web_intent("corrige este error de sintaxis"));
    }

    #[test]
    fn test_has_web_intent_ignores_code_shaped_prompts() {
        // Dotted tokens that are not hostnames.
        assert!(!has_web_intent("refactor this .iter() chain in tree.rs"));
        assert!(!has_web_intent("rename config.toml and rerun build.rs"));
        assert!(!has_web_intent("rename the file to main.io"));
        // Markers embedded in identifiers.
        assert!(!has_web_intent("fix the linker error in build.rs"));
        assert!(!has_web_intent("aggiorna la webpack config"));
        assert!(!has_web_intent("replace this symlink with a copy"));
        // Verbs with no external reference material.
        assert!(!has_web_intent("find the null pointer dereference"));
        assert!(!has_web_intent("ich versuche den fehler zu finden"));
        assert!(!has_web_intent("fetch the user record from the database"));
        // "browser" is everyday vocabulary in a browser-engine repo.
        assert!(!has_web_intent("fix the browser crash on paint"));
        assert!(!has_web_intent("il browser non renderizza il box-shadow"));
    }

    #[test]
    fn test_browse_still_matches_with_a_topic() {
        assert!(has_web_intent("browse the react documentation"));
        assert!(has_web_intent("naviga la guida di github"));
    }

    #[test]
    fn test_has_web_intent_still_matches_real_hosts() {
        assert!(has_web_intent("open example.com/docs"));
        // A real .io address still gets through by its scheme.
        assert!(has_web_intent("open https://fly.io/docs"));
        assert!(has_web_intent("apri telemaco.dev per favore"));
        assert!(has_web_intent("check docs.rs/serde_json — no wait, example.org"));
    }

    #[test]
    fn test_process_hook_input_claude_codex() {
        // Web intent present -> plain text directive
        let input_web = r#"{"prompt": "search the web for the new Next.js docs", "cwd": "/tmp"}"#;
        let out_web = process_hook_input_as(input_web, OutputFormat::Text);
        assert!(out_web.is_some());
        let res = out_web.unwrap();
        assert!(res.contains("<telemaco_directive>"));

        // No web intent -> None (no injection)
        let input_noweb = r#"{"prompt": "refactor this function", "cwd": "/tmp"}"#;
        let out_noweb = process_hook_input_as(input_noweb, OutputFormat::Text);
        assert!(out_noweb.is_none());
    }

    #[test]
    fn test_transcript_tolerates_spacing_and_multibyte() {
        let dir = std::env::temp_dir().join(format!("telemaco_tr_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);

        // Pretty-printed JSON on one line: the old byte match missed this.
        let spaced = dir.join("spaced.jsonl");
        std::fs::write(
            &spaced,
            "{\"type\": \"USER_INPUT\", \"content\": \"cerca online la documentazione\"}\n",
        )
        .unwrap();
        assert_eq!(
            extract_prompt_from_transcript(spaced.to_str().unwrap()).as_deref(),
            Some("cerca online la documentazione")
        );

        // A multibyte character straddling the 64 KiB tail boundary used to
        // make the whole read fail.
        let big = dir.join("big.jsonl");
        let user = "{\"type\":\"USER_INPUT\",\"content\":\"cerca online la documentazione\"}\n";
        let tail_len = 64 * 1024 - 2 - user.len();
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend(b"a".repeat(999));
        bytes.push(b'\n');
        bytes.extend("\u{20ac}".as_bytes());
        bytes.extend(b"y".repeat(tail_len));
        bytes.push(b'\n');
        bytes.extend(user.as_bytes());
        std::fs::write(&big, &bytes).unwrap();
        assert_eq!(
            extract_prompt_from_transcript(big.to_str().unwrap()).as_deref(),
            Some("cerca online la documentazione")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_json_output_carries_the_directive_as_additional_context() {
        let input = r#"{"prompt": "search the web for the new Next.js docs"}"#;
        let out = process_hook_input_as(input, OutputFormat::Json).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["hookSpecificOutput"]["hookEventName"], "UserPromptSubmit");
        assert!(parsed["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .contains("<telemaco_directive>"));

        // No intent still means no output at all, in either format.
        let quiet = r#"{"prompt": "refactor this function"}"#;
        assert!(process_hook_input_as(quiet, OutputFormat::Json).is_none());
    }

    #[test]
    fn kiro_reads_the_prompt_from_the_environment() {
        // What Kiro sends on stdin for UserPromptSubmit: session context, no
        // prompt. The prompt is in USER_PROMPT.
        let input = r#"{"hook_event_name":"UserPromptSubmit","cwd":"/repo"}"#;
        assert!(process_hook_input_with(input, OutputFormat::Text, None).is_none());

        let out = process_hook_input_with(
            input,
            OutputFormat::Text,
            Some("read the docs at https://example.com"),
        )
        .unwrap();
        assert!(out.contains("<telemaco_directive>"));

        // Still nothing to say when the prompt is not web-bound.
        assert!(
            process_hook_input_with(input, OutputFormat::Text, Some("rename this variable"))
                .is_none()
        );

        // A prompt on stdin wins over the environment.
        let with_prompt = r#"{"hook_event_name":"UserPromptSubmit","prompt":"rename this variable"}"#;
        assert!(process_hook_input_with(
            with_prompt,
            OutputFormat::Text,
            Some("open https://example.com")
        )
        .is_none());
    }

    #[test]
    fn poolside_output_uses_its_snake_case_decision_object() {
        let input = r#"{"hook_event_name":"UserPromptSubmit","prompt":"fetch https://example.com"}"#;
        let out = process_hook_input_as(input, OutputFormat::Poolside).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(parsed["hook_specific_output"]["additional_context"]
            .as_str()
            .unwrap()
            .contains("TELEMACO"));
        // camelCase would be ignored in silence, which is the failure mode the
        // docs warn about.
        assert!(parsed["hookSpecificOutput"].is_null());

        let quiet = r#"{"hook_event_name":"UserPromptSubmit","prompt":"rename this variable"}"#;
        assert!(process_hook_input_as(quiet, OutputFormat::Poolside).is_none());
    }

    #[test]
    fn hermes_output_uses_its_own_context_key() {
        // Hermes' pre_llm_call reads `{"context": ...}` and puts the submitted
        // message in `extra.user_message`, not in `prompt`.
        let input = r#"{"hook_event_name":"pre_llm_call","tool_name":null,"extra":{"user_message":"search the web for tokio docs"}}"#;
        let out = process_hook_input_as(input, OutputFormat::Hermes).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(parsed["context"].as_str().unwrap().contains("TELEMACO"));

        // No web intent, no injection.
        let quiet = r#"{"hook_event_name":"pre_llm_call","extra":{"user_message":"rename this variable"}}"#;
        assert!(process_hook_input_as(quiet, OutputFormat::Hermes).is_none());
    }

    #[test]
    fn json_output_names_the_event_that_fired() {
        // Gemini CLI injects context from BeforeAgent. Answering every agent
        // with "UserPromptSubmit" named an event that never fired.
        let input = r#"{"hook_event_name":"BeforeAgent","prompt":"search the web for rust async"}"#;
        let out = process_hook_input_as(input, OutputFormat::Json).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["hookSpecificOutput"]["hookEventName"], "BeforeAgent");

        // Without one on stdin the Claude Code / Qwen name is still the default.
        let bare = r#"{"prompt":"search the web for rust async"}"#;
        let out = process_hook_input_as(bare, OutputFormat::Json).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["hookSpecificOutput"]["hookEventName"], "UserPromptSubmit");
    }

    #[test]
    fn test_process_hook_input_antigravity() {
        let temp_dir = std::env::temp_dir().join(format!("telemaco_hook_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let trans_path = temp_dir.join("transcript.jsonl");

        std::fs::write(
            &trans_path,
            "{\"type\":\"PLANNER_RESPONSE\",\"content\":\"ok\"}\n{\"type\":\"USER_INPUT\",\"content\":\"cerca online la documentazione ufficiale\"}\n"
        ).unwrap();

        let input = format!(
            r#"{{"conversationId": "test-123", "transcriptPath": "{}"}}"#,
            trans_path.display()
        );
        let out = process_hook_input_as(&input, OutputFormat::Text);
        assert!(out.is_some());
        let json_val: serde_json::Value = serde_json::from_str(&out.unwrap()).unwrap();
        assert!(json_val["injectSteps"].is_array());
        let msg = json_val["injectSteps"][0]["ephemeralMessage"].as_str().unwrap();
        assert!(msg.contains("<telemaco_directive>"));

        // Antigravity negative
        let trans_neg_path = temp_dir.join("transcript_neg.jsonl");
        std::fs::write(
            &trans_neg_path,
            "{\"type\":\"USER_INPUT\",\"content\":\"fix compilation error\"}\n"
        ).unwrap();
        let input_neg = format!(
            r#"{{"conversationId": "test-456", "transcriptPath": "{}"}}"#,
            trans_neg_path.display()
        );
        let out_neg = process_hook_input_as(&input_neg, OutputFormat::Text);
        assert_eq!(out_neg.as_deref(), Some("{}"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
