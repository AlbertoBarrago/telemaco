//! Tailcat transport, driving the external `tailcat` CLI.
//!
//! Tailcat provides the WireGuard tunnel, NAT traversal and DERP fallback;
//! Telemaco never reimplements any of it. The client side runs
//! `tailcat <addr> <port>`, which pipes its own stdio to that port on the
//! server. The server side (a later phase) runs `tailcat serve -- <agent>`,
//! Tailcat's inetd-style `exec` service, which starts the agent per
//! connection with the connection as its stdio. Neither side opens a TCP
//! listener, so no other local user can reach the agent.
//!
//! Verified against tailcat v0.7.0 (`cmd/tailcat/tailcat.go`).

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::process::run_captured;
use super::{Connection, ProcessSpec, Transport, TransportError, TransportKind};
use crate::address::TailcatAddress;

/// Oldest tailcat release this integration was checked against.
pub const MIN_TAILCAT_VERSION: (u32, u32, u32) = (0, 7, 0);

/// Tunnel port the client dials for the agent. Tailcat's `exec` service
/// answers on every port not explicitly forwarded, so the value only has to
/// be stable and must never be offered for port forwarding.
pub const AGENT_PORT: u16 = 7431;

const VERSION_TIMEOUT: Duration = Duration::from_secs(10);

/// A located `tailcat` executable.
#[derive(Debug, Clone)]
pub struct TailcatCli {
    program: PathBuf,
}

impl TailcatCli {
    /// Finds `tailcat`: the explicit override if given (it must exist), else
    /// the first executable named `tailcat` on `PATH`.
    pub fn locate(override_path: Option<&Path>) -> Result<Self, TransportError> {
        Self::locate_in(override_path, std::env::var_os("PATH").as_deref())
    }

    /// [`TailcatCli::locate`] with an explicit `PATH` value, for tests.
    pub fn locate_in(
        override_path: Option<&Path>,
        path_var: Option<&OsStr>,
    ) -> Result<Self, TransportError> {
        if let Some(p) = override_path {
            return if is_executable(p) {
                Ok(Self {
                    program: p.to_path_buf(),
                })
            } else {
                Err(TransportError::InvalidTailcatOverride(p.to_path_buf()))
            };
        }
        let path_var = path_var.ok_or(TransportError::TailcatUnavailable)?;
        std::env::split_paths(path_var)
            .flat_map(|dir| candidate_names().map(move |name| dir.join(name)))
            .find(|p| is_executable(p))
            .map(|program| Self { program })
            .ok_or(TransportError::TailcatUnavailable)
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    /// A spec for this binary with no arguments yet.
    pub fn spec(&self) -> ProcessSpec {
        ProcessSpec::new(&self.program)
    }

    /// Runs `tailcat version`.
    pub async fn version(&self) -> Result<TailcatVersion, TransportError> {
        let out = run_captured(&self.spec().arg("version"), VERSION_TIMEOUT).await?;
        Ok(TailcatVersion::parse(&out.stdout))
    }

    /// Fails on a release older than [`MIN_TAILCAT_VERSION`]. A version
    /// string that does not parse (a source build reports `(devel)`) is let
    /// through with a warning: it says nothing about security, only age.
    pub async fn ensure_supported(&self) -> Result<TailcatVersion, TransportError> {
        let version = self.version().await?;
        match version.numeric {
            Some(v) if v < MIN_TAILCAT_VERSION => Err(TransportError::UnsupportedTailcatVersion {
                found: version.raw,
                required: format_version(MIN_TAILCAT_VERSION),
            }),
            Some(_) => Ok(version),
            None => {
                tracing::warn!(version = %version.raw, "unrecognized tailcat version; continuing");
                Ok(version)
            }
        }
    }
}

/// Output of `tailcat version`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailcatVersion {
    pub raw: String,
    pub numeric: Option<(u32, u32, u32)>,
}

impl TailcatVersion {
    /// Accepts `v0.7.0`, `0.7.0`, and suffixed forms like
    /// `v0.7.1-0.20260920-abcdef`; anything else keeps only `raw`.
    pub fn parse(output: &str) -> Self {
        let raw = output.trim().to_string();
        let core = raw.strip_prefix('v').unwrap_or(&raw);
        let core = core.split(['-', '+', ' ']).next().unwrap_or("");
        let mut parts = core.split('.').map(|p| p.parse::<u32>().ok());
        let numeric = match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(Some(a)), Some(Some(b)), Some(Some(c)), None) => Some((a, b, c)),
            _ => None,
        };
        Self { raw, numeric }
    }
}

impl fmt::Display for TailcatVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

fn format_version((a, b, c): (u32, u32, u32)) -> String {
    format!("v{a}.{b}.{c}")
}

/// Client side of the Tailcat transport: one `tailcat <addr> <port>` child
/// per connection.
#[derive(Debug, Clone)]
pub struct TailcatTransport {
    cli: TailcatCli,
    addr: TailcatAddress,
    port: u16,
}

impl TailcatTransport {
    pub fn new(cli: TailcatCli, addr: TailcatAddress) -> Self {
        Self {
            cli,
            addr,
            port: AGENT_PORT,
        }
    }

    pub fn cli(&self) -> &TailcatCli {
        &self.cli
    }

    pub fn address(&self) -> &TailcatAddress {
        &self.addr
    }

    /// The argv for one agent session. The address is validated to start
    /// with `tc` and be pure base64url, so it can never be read as a flag.
    pub(crate) fn client_spec(&self) -> ProcessSpec {
        self.cli
            .spec()
            .secret(self.addr.clone())
            .arg(self.port.to_string())
    }
}

impl Transport for TailcatTransport {
    fn kind(&self) -> TransportKind {
        TransportKind::Tailcat
    }

    async fn connect(&self) -> Result<Connection, TransportError> {
        tracing::debug!(remote = %self.addr, "connecting over tailcat");
        Connection::spawn(self.client_spec())
    }
}

fn candidate_names() -> impl Iterator<Item = &'static str> {
    let names: &[&str] = if cfg!(windows) {
        &["tailcat.exe", "tailcat"]
    } else {
        &["tailcat"]
    };
    names.iter().copied()
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parsing() {
        assert_eq!(TailcatVersion::parse("v0.7.0\n").numeric, Some((0, 7, 0)));
        assert_eq!(TailcatVersion::parse("0.10.2").numeric, Some((0, 10, 2)));
        assert_eq!(
            TailcatVersion::parse("v0.7.1-0.20260920-abcdef").numeric,
            Some((0, 7, 1))
        );
        assert_eq!(TailcatVersion::parse("(devel)").numeric, None);
        assert_eq!(TailcatVersion::parse("1.2").numeric, None);
        assert_eq!(TailcatVersion::parse("1.2.3.4").numeric, None);
    }

    #[test]
    fn missing_from_path_gives_the_install_hint() {
        let err = TailcatCli::locate_in(None, Some(OsStr::new("/nonexistent-a:/nonexistent-b")))
            .unwrap_err();
        assert!(matches!(err, TransportError::TailcatUnavailable));
        let msg = err.to_string();
        assert!(msg.contains("Tailcat transport is unavailable"), "{msg}");
        assert!(msg.contains("Install tailcat"), "{msg}");
    }

    #[test]
    fn no_path_at_all_is_unavailable() {
        let err = TailcatCli::locate_in(None, None).unwrap_err();
        assert!(matches!(err, TransportError::TailcatUnavailable));
    }

    #[cfg(unix)]
    mod unix {
        use super::*;
        use crate::address::tests::SAMPLE;
        use crate::transport::test_support::fake_bin;
        use tokio::io::AsyncReadExt;

        #[test]
        fn found_on_path() {
            let bin = fake_bin("tailcat", "exit 0");
            let dir = bin.parent().unwrap();
            let path = std::env::join_paths(["/nonexistent", dir.to_str().unwrap()]).unwrap();
            let cli = TailcatCli::locate_in(None, Some(&path)).unwrap();
            assert_eq!(cli.program(), bin);
        }

        #[test]
        fn non_executable_file_on_path_is_skipped() {
            let bin = fake_bin("tailcat", "exit 0");
            std::fs::set_permissions(&bin, std::os::unix::fs::PermissionsExt::from_mode(0o644))
                .unwrap();
            let err =
                TailcatCli::locate_in(None, Some(bin.parent().unwrap().as_os_str())).unwrap_err();
            assert!(matches!(err, TransportError::TailcatUnavailable));
        }

        #[tokio::test]
        async fn old_version_is_rejected() {
            let cli = TailcatCli::locate(Some(&fake_bin("tailcat", "echo v0.5.0"))).unwrap();
            let err = cli.ensure_supported().await.unwrap_err();
            assert!(err.to_string().contains("v0.5.0"), "{err}");
            assert!(err.to_string().contains("v0.7.0"), "{err}");
        }

        #[tokio::test]
        async fn current_and_devel_versions_are_accepted() {
            for out in ["echo v0.7.0", "echo v1.0.0", "echo '(devel)'"] {
                let cli = TailcatCli::locate(Some(&fake_bin("tailcat", out))).unwrap();
                cli.ensure_supported().await.unwrap();
            }
        }

        #[tokio::test]
        async fn version_failure_keeps_tailcat_stderr() {
            let bin = fake_bin(
                "tailcat",
                "echo 'flag provided but not defined' >&2; exit 2",
            );
            let cli = TailcatCli::locate(Some(&bin)).unwrap();
            let err = cli.version().await.unwrap_err();
            assert!(
                err.to_string().contains("flag provided but not defined"),
                "{err}"
            );
        }

        #[tokio::test]
        async fn client_argv_is_address_then_agent_port() {
            let bin = fake_bin("tailcat", r#"for a in "$@"; do printf '[%s]\n' "$a"; done"#);
            let cli = TailcatCli::locate(Some(&bin)).unwrap();
            let t = TailcatTransport::new(cli, TailcatAddress::parse(SAMPLE).unwrap());
            let mut conn = t.connect().await.unwrap();
            let mut out = String::new();
            conn.read_to_string(&mut out).await.unwrap();
            assert_eq!(out, format!("[{SAMPLE}]\n[{AGENT_PORT}]\n"));
            conn.close(Duration::from_secs(5)).await.unwrap();
        }

        #[tokio::test]
        async fn early_exit_surfaces_as_failure_with_stderr() {
            let bin = fake_bin(
                "tailcat",
                r#"echo "tailcat: handshake with $1 timed out" >&2; exit 1"#,
            );
            let cli = TailcatCli::locate(Some(&bin)).unwrap();
            let t = TailcatTransport::new(cli, TailcatAddress::parse(SAMPLE).unwrap());
            let mut conn = t.connect().await.unwrap();
            let mut buf = Vec::new();
            conn.read_to_end(&mut buf).await.unwrap();
            assert!(buf.is_empty());
            let msg = conn.failure().await.to_string();
            assert!(
                msg.contains("handshake with tcomFw...**** timed out"),
                "{msg}"
            );
            assert!(!msg.contains(SAMPLE), "{msg}");
        }
    }
}
