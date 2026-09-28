//! Transports: how bytes reach a remote Telemaco agent.
//!
//! Every transport yields a [`Connection`], an `AsyncRead + AsyncWrite` byte
//! stream. The remote protocol is written against that stream only, so it
//! cannot tell Tailcat from a local agent or a future SSH transport. The one
//! place that maps a target to a transport is [`select_transport`].

mod local;
mod process;
pub mod tailcat;

use std::fmt;
use std::future::Future;
use std::path::PathBuf;

use crate::address::{AddressError, TailcatAddress};

pub use local::LocalTransport;
pub use process::{Arg, CapturedOutput, Connection, ProcessSpec};
pub use tailcat::{TailcatCli, TailcatTransport, TailcatVersion};

/// A way to open a byte stream to a remote agent.
pub trait Transport {
    /// Which transport this is, for status output and diagnostics.
    fn kind(&self) -> TransportKind;

    /// Opens a new stream to the agent. Each call is an independent session.
    fn connect(&self) -> impl Future<Output = Result<Connection, TransportError>> + Send;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    Tailcat,
    Local,
}

impl fmt::Display for TransportKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            TransportKind::Tailcat => "tailcat",
            TransportKind::Local => "local",
        })
    }
}

/// Transport failures. Messages carry the underlying tool's own stderr
/// (redacted) rather than a generic summary, since that is usually the only
/// useful diagnostic.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("Tailcat transport is unavailable: `tailcat` was not found in PATH.\n\nInstall tailcat or use another configured transport.")]
    TailcatUnavailable,
    #[error("TELEMACO_TAILCAT_BIN points to {}, which is not an executable file", .0.display())]
    InvalidTailcatOverride(PathBuf),
    #[error("tailcat {found} is not supported; Telemaco needs tailcat {required} or newer")]
    UnsupportedTailcatVersion { found: String, required: String },
    #[error("the local transport is not configured in this build")]
    LocalUnavailable,
    #[error("failed to start {program}: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{program} exited with {status}{}", stderr_suffix(.stderr))]
    Exited {
        program: String,
        status: String,
        stderr: String,
    },
    #[error("{program} did not finish within {seconds}s")]
    Timeout { program: String, seconds: u64 },
    #[error("transport I/O error: {0}")]
    Io(#[from] std::io::Error),
}

fn stderr_suffix(stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!(": {trimmed}")
    }
}

/// Where a remote command should go, as typed by the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteTarget {
    /// A tailcat address (`tc...`).
    Tailcat(TailcatAddress),
    /// An agent on this machine, reached without any network hop.
    Local,
}

/// Why a target string names no known transport.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("expected a tailcat address (tc...) or `local`: {0}")]
pub struct TargetError(#[from] AddressError);

impl RemoteTarget {
    pub fn parse(input: &str) -> Result<Self, TargetError> {
        if input.trim() == "local" {
            return Ok(Self::Local);
        }
        Ok(Self::Tailcat(TailcatAddress::parse(input)?))
    }
}

/// Inputs transport selection needs from the outside world. Kept explicit so
/// tests never depend on the real `PATH` or on a real `tailcat` binary.
#[derive(Debug, Clone, Default)]
pub struct TransportConfig {
    /// Explicit `tailcat` binary, from `TELEMACO_TAILCAT_BIN`. When set it
    /// must exist: a wrong override is an error, not a silent PATH fallback.
    pub tailcat_bin: Option<PathBuf>,
    /// How to launch a local agent, supplied by the CLI (which knows its own
    /// executable and subcommand layout).
    pub local_agent: Option<ProcessSpec>,
}

impl TransportConfig {
    /// Reads `TELEMACO_TAILCAT_BIN`; an empty value counts as unset.
    pub fn from_env() -> Self {
        Self {
            tailcat_bin: std::env::var_os("TELEMACO_TAILCAT_BIN")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
            local_agent: None,
        }
    }
}

/// Static dispatch over the known transports, so callers can hold "some
/// transport" without boxing an `impl Future` trait.
#[derive(Debug)]
pub enum AnyTransport {
    Tailcat(TailcatTransport),
    Local(LocalTransport),
}

impl Transport for AnyTransport {
    fn kind(&self) -> TransportKind {
        match self {
            AnyTransport::Tailcat(t) => t.kind(),
            AnyTransport::Local(t) => t.kind(),
        }
    }

    async fn connect(&self) -> Result<Connection, TransportError> {
        match self {
            AnyTransport::Tailcat(t) => t.connect().await,
            AnyTransport::Local(t) => t.connect().await,
        }
    }
}

/// Maps a target to its transport. Locating `tailcat` happens here, so a
/// missing binary is reported before any connection attempt.
pub fn select_transport(
    target: RemoteTarget,
    config: &TransportConfig,
) -> Result<AnyTransport, TransportError> {
    match target {
        RemoteTarget::Tailcat(addr) => {
            let cli = TailcatCli::locate(config.tailcat_bin.as_deref())?;
            Ok(AnyTransport::Tailcat(TailcatTransport::new(cli, addr)))
        }
        RemoteTarget::Local => config
            .local_agent
            .clone()
            .map(|spec| AnyTransport::Local(LocalTransport::new(spec)))
            .ok_or(TransportError::LocalUnavailable),
    }
}

#[cfg(all(test, unix))]
pub(crate) mod test_support {
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Writes an executable `/bin/sh` script standing in for `tailcat`, so
    /// subprocess behavior is tested without any network or real binary.
    pub(crate) fn fake_bin(name: &str, body: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "telemaco-remote-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address::tests::SAMPLE;

    #[test]
    fn target_parsing_distinguishes_local_and_tailcat() {
        assert_eq!(RemoteTarget::parse("local").unwrap(), RemoteTarget::Local);
        assert!(matches!(
            RemoteTarget::parse(SAMPLE).unwrap(),
            RemoteTarget::Tailcat(_)
        ));
        let err = RemoteTarget::parse("my-server").unwrap_err();
        assert!(err.to_string().contains("tailcat address"));
    }

    #[test]
    fn local_target_without_agent_config_is_an_error() {
        let err = select_transport(RemoteTarget::Local, &TransportConfig::default()).unwrap_err();
        assert!(matches!(err, TransportError::LocalUnavailable));
    }

    #[test]
    fn local_target_selects_local_transport() {
        let config = TransportConfig {
            tailcat_bin: None,
            local_agent: Some(ProcessSpec::new("/bin/cat")),
        };
        let t = select_transport(RemoteTarget::Local, &config).unwrap();
        assert_eq!(t.kind(), TransportKind::Local);
    }

    #[test]
    fn missing_tailcat_override_is_reported_not_ignored() {
        let config = TransportConfig {
            tailcat_bin: Some(PathBuf::from("/nonexistent/tailcat")),
            local_agent: None,
        };
        let target = RemoteTarget::parse(SAMPLE).unwrap();
        let err = select_transport(target, &config).unwrap_err();
        assert!(matches!(err, TransportError::InvalidTailcatOverride(_)));
    }

    #[cfg(unix)]
    #[test]
    fn tailcat_target_selects_tailcat_transport() {
        let bin = test_support::fake_bin("tailcat", "exit 0");
        let config = TransportConfig {
            tailcat_bin: Some(bin),
            local_agent: None,
        };
        let target = RemoteTarget::parse(SAMPLE).unwrap();
        let t = select_transport(target, &config).unwrap();
        assert_eq!(t.kind(), TransportKind::Tailcat);
        // The selected transport must not leak the address through Debug.
        assert!(!format!("{t:?}").contains(SAMPLE));
    }
}
