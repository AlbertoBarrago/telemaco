//! Tailcat address handling.
//!
//! A tailcat address (`tc` + base64url CBOR) carries the server's WireGuard
//! public key and, by default, a pre-shared key. Knowing it is what lets a
//! client connect, so it is treated as a capability: `Debug` and `Display`
//! are redacted, and the full value is only reachable through
//! [`TailcatAddress::expose`], used to build the `tailcat` argv.

use std::fmt;
use std::str::FromStr;

/// Characters kept visible by the redacted form, including the `tc` prefix.
const VISIBLE_PREFIX: usize = 6;
/// Shortest address accepted. Real addresses are 50+ characters; anything
/// much shorter is a typo, not a key.
const MIN_LEN: usize = 20;
/// Longest address accepted. Resolved addresses embed DERP node info and run
/// to a couple of hundred characters; the cap only bounds hostile input.
const MAX_LEN: usize = 2048;

/// A validated, redacted-by-default tailcat address.
#[derive(Clone, PartialEq, Eq)]
pub struct TailcatAddress(String);

/// Why a string is not a tailcat address. Never echoes the input, which may
/// be a real address with one wrong character.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AddressError {
    #[error("a tailcat address starts with \"tc\"")]
    MissingPrefix,
    #[error("tailcat address is too short ({0} characters)")]
    TooShort(usize),
    #[error("tailcat address is too long ({0} characters, max {MAX_LEN})")]
    TooLong(usize),
    #[error("tailcat address contains a character outside base64url at position {0}")]
    InvalidCharacter(usize),
}

impl TailcatAddress {
    /// Validates the shape of an address without decoding it: `tc` prefix,
    /// base64url alphabet, bounded length. Tailcat itself does the real
    /// decoding and reports its own errors.
    pub fn parse(input: &str) -> Result<Self, AddressError> {
        let s = input.trim();
        if !s.starts_with("tc") {
            return Err(AddressError::MissingPrefix);
        }
        if s.len() < MIN_LEN {
            return Err(AddressError::TooShort(s.len()));
        }
        if s.len() > MAX_LEN {
            return Err(AddressError::TooLong(s.len()));
        }
        if let Some(pos) = s
            .bytes()
            .position(|b| !(b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
        {
            return Err(AddressError::InvalidCharacter(pos));
        }
        Ok(Self(s.to_string()))
    }

    /// The full address. Only for handing to the `tailcat` process or for the
    /// one line `remote serve` prints so the user can share it.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// The redacted form, e.g. `tcAbCd...****`.
    pub fn redacted(&self) -> String {
        format!("{}...****", &self.0[..VISIBLE_PREFIX])
    }

    /// Replaces every occurrence of this address in `text` with its redacted
    /// form. Tailcat error messages can quote the address they failed on.
    pub fn redact_in(&self, text: &str) -> String {
        text.replace(&self.0, &self.redacted())
    }
}

/// Redacts anything shaped like a tailcat address in `text`, for output
/// whose addresses are not known in advance (e.g. `tailcat serve` announces
/// its fresh address on stderr before Telemaco has read it from stdout).
/// Over-redacting a long `tc...` identifier that is not an address is fine.
pub fn redact_tailcat_tokens(text: &str) -> String {
    let is_addr_char = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(|c: char| is_addr_char(c)) {
        out.push_str(&rest[..start]);
        let token_len = rest[start..]
            .find(|c: char| !is_addr_char(c))
            .unwrap_or(rest.len() - start);
        let token = &rest[start..start + token_len];
        match TailcatAddress::parse(token) {
            Ok(addr) => out.push_str(&addr.redacted()),
            Err(_) => out.push_str(token),
        }
        rest = &rest[start + token_len..];
    }
    out.push_str(rest);
    out
}

impl FromStr for TailcatAddress {
    type Err = AddressError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl fmt::Debug for TailcatAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TailcatAddress")
            .field(&self.redacted())
            .finish()
    }
}

impl fmt::Display for TailcatAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.redacted())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Shape of a real address (from the Tailcat README), not a live one.
    pub(crate) const SAMPLE: &str = "tcomFwWCCcjS5nKNqAod034nWoJZW0LZqDhhC8U_dKdnDRYQ8uNGFpGQEu";

    #[test]
    fn parses_a_well_formed_address() {
        let addr = TailcatAddress::parse(SAMPLE).unwrap();
        assert_eq!(addr.expose(), SAMPLE);
    }

    #[test]
    fn trims_surrounding_whitespace() {
        let addr = TailcatAddress::parse(&format!("  {SAMPLE}\n")).unwrap();
        assert_eq!(addr.expose(), SAMPLE);
    }

    #[test]
    fn debug_and_display_never_show_the_full_address() {
        let addr = TailcatAddress::parse(SAMPLE).unwrap();
        let debug = format!("{addr:?}");
        let display = addr.to_string();
        assert_eq!(display, "tcomFw...****");
        assert!(debug.contains("tcomFw...****"));
        assert!(!debug.contains(SAMPLE));
        assert!(!display.contains(&SAMPLE[VISIBLE_PREFIX..]));
    }

    #[test]
    fn redact_in_scrubs_every_occurrence() {
        let addr = TailcatAddress::parse(SAMPLE).unwrap();
        let text = format!("dial {SAMPLE}: timeout (addr {SAMPLE})");
        let out = addr.redact_in(&text);
        assert!(!out.contains(SAMPLE));
        assert_eq!(out.matches("tcomFw...****").count(), 2);
    }

    #[test]
    fn rejects_malformed_input() {
        assert_eq!(
            TailcatAddress::parse("example.com"),
            Err(AddressError::MissingPrefix)
        );
        assert_eq!(
            TailcatAddress::parse("tcshort"),
            Err(AddressError::TooShort(7))
        );
        let long = format!("tc{}", "a".repeat(MAX_LEN));
        assert_eq!(
            TailcatAddress::parse(&long),
            Err(AddressError::TooLong(MAX_LEN + 2))
        );
        // Shell metacharacters must never make it into an argv slot.
        let hostile = format!("{SAMPLE};rm -rf ~");
        assert_eq!(
            TailcatAddress::parse(&hostile),
            Err(AddressError::InvalidCharacter(SAMPLE.len()))
        );
    }

    #[test]
    fn token_redaction_finds_unknown_addresses() {
        let line = format!("# 🐈 Server listening with new address: {SAMPLE}\n");
        let out = redact_tailcat_tokens(&line);
        assert_eq!(
            out,
            "# 🐈 Server listening with new address: tcomFw...****\n"
        );
        // Ordinary words, including short `tc` ones, pass through untouched.
        let plain = "tcp dial to tcfoo failed: context deadline exceeded";
        assert_eq!(redact_tailcat_tokens(plain), plain);
    }

    #[test]
    fn errors_do_not_echo_the_input() {
        let bad = format!("{SAMPLE}!");
        let err = TailcatAddress::parse(&bad).unwrap_err().to_string();
        assert!(!err.contains(&SAMPLE[VISIBLE_PREFIX..]));
    }
}
