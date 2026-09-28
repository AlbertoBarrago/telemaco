//! Port forwarding over Tailcat: `tailcat forward --bind=<ip> <addr> L:R`.
//!
//! Tailcat owns the listener and the tunnel; Telemaco only starts it, reads
//! the `# forwarding <local> -> remote <target>` line it prints once the
//! listener is up, and tears it down. The server side exposes a port only if
//! `remote serve --forward-port` named it.

use std::collections::VecDeque;
use std::net::{IpAddr, SocketAddr};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::process::Child;
use tokio::sync::mpsc;

use super::tailcat_server::{exit_report, relay_stderr};
use super::{ProcessSpec, TailcatCli, TransportError};
use crate::address::TailcatAddress;

/// What to forward: a local listener to a port on the remote's localhost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForwardRequest {
    pub remote_port: u16,
    /// `0` asks the OS for a free port.
    pub local_port: u16,
    /// Listen address. Loopback unless the user explicitly chose otherwise.
    pub bind: IpAddr,
}

/// A running forwarder. Dropping it kills the forwarding process.
#[derive(Debug)]
pub struct Forwarder {
    child: Child,
    local: SocketAddr,
    spec: ProcessSpec,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
}

impl Forwarder {
    pub(crate) fn spec(
        cli: &TailcatCli,
        addr: &TailcatAddress,
        req: &ForwardRequest,
    ) -> ProcessSpec {
        cli.spec()
            .arg("forward")
            .arg(format!("--bind={}", req.bind))
            .secret(addr.clone())
            .arg(format!("{}:{}", req.local_port, req.remote_port))
    }

    /// Starts tailcat and waits for it to report the local listener.
    pub(crate) async fn start(
        spec: ProcessSpec,
        startup: Duration,
    ) -> Result<(Self, mpsc::Receiver<String>), TransportError> {
        let mut child = spec
            .command()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| spec.spawn_error(e))?;
        let Some(stderr) = child.stderr.take() else {
            return Err(spec.spawn_error(std::io::Error::other("child stderr was not piped")));
        };
        let stderr_tail = Arc::new(Mutex::new(VecDeque::new()));
        let (tx, mut raw) = mpsc::channel(256);
        tokio::spawn(relay_stderr(stderr, tx, Arc::clone(&stderr_tail)));

        // Watch the relayed lines for the listener announcement, then hand
        // the remaining lines to the caller through a fresh channel.
        let wait_for_listener = async {
            while let Some(line) = raw.recv().await {
                if let Some(local) = parse_listening(&line) {
                    return Some(local);
                }
            }
            None
        };
        match tokio::time::timeout(startup, wait_for_listener).await {
            Ok(Some(local)) => {
                let (tx, rx) = mpsc::channel(256);
                tokio::spawn(async move {
                    while let Some(line) = raw.recv().await {
                        if tx.send(line).await.is_err() {
                            break;
                        }
                    }
                });
                Ok((
                    Self {
                        child,
                        local,
                        spec,
                        stderr_tail,
                    },
                    rx,
                ))
            }
            Ok(None) => Err(exit_report(&mut child, &spec, &stderr_tail).await),
            Err(_) => {
                let _ = child.kill().await;
                Err(TransportError::Timeout {
                    program: spec.display_name(),
                    seconds: startup.as_secs(),
                })
            }
        }
    }

    /// The address local clients connect to (with the port the OS picked,
    /// if the request asked for port 0).
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    /// Resolves when the forwarder exits, with its status and recent stderr.
    pub async fn wait(&mut self) -> TransportError {
        exit_report(&mut self.child, &self.spec, &self.stderr_tail).await
    }

    /// Stops forwarding and reaps the process.
    pub async fn shutdown(mut self) -> Result<(), TransportError> {
        match self.child.kill().await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::InvalidInput => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

/// Parses `# forwarding 127.0.0.1:54321 -> remote localhost:5432`.
fn parse_listening(line: &str) -> Option<SocketAddr> {
    let rest = line.strip_prefix("# forwarding ")?;
    let (local, _) = rest.split_once(" -> ")?;
    local.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listener_line_parsing() {
        assert_eq!(
            parse_listening("# forwarding 127.0.0.1:54321 -> remote localhost:5432"),
            Some("127.0.0.1:54321".parse().unwrap())
        );
        assert_eq!(
            parse_listening("# forwarding [::1]:8080 -> remote localhost:8080"),
            Some("[::1]:8080".parse().unwrap())
        );
        assert_eq!(
            parse_listening("# Selected bootstrap relay region 303"),
            None
        );
        assert_eq!(parse_listening("# forwarding garbage -> remote x"), None);
    }

    #[cfg(unix)]
    mod unix {
        use super::*;
        use crate::address::tests::SAMPLE;
        use crate::transport::test_support::fake_bin;
        use crate::transport::{Arg, TailcatTransport, Transport};

        fn req() -> ForwardRequest {
            ForwardRequest {
                remote_port: 5432,
                local_port: 0,
                bind: "127.0.0.1".parse().unwrap(),
            }
        }

        #[test]
        fn argv_is_bind_then_address_then_mapping() {
            let cli = TailcatCli::locate(Some(&fake_bin("tailcat", "exit 0"))).unwrap();
            let spec = Forwarder::spec(&cli, &TailcatAddress::parse(SAMPLE).unwrap(), &req());
            let argv: Vec<String> = spec
                .args
                .iter()
                .map(|a| match a {
                    Arg::Plain(s) => s.to_string_lossy().into_owned(),
                    Arg::Secret(_) => "<addr>".into(),
                })
                .collect();
            assert_eq!(argv, ["forward", "--bind=127.0.0.1", "<addr>", "0:5432"]);
        }

        #[tokio::test]
        async fn start_reports_the_listener_and_stops_cleanly() {
            let body = r##"[ "$1" = version ] && { echo v0.7.0; exit 0; }
echo "# forwarding 127.0.0.1:54321 -> remote localhost:5432" >&2
exec sleep 30"##;
            let cli = TailcatCli::locate(Some(&fake_bin("tailcat", body))).unwrap();
            let t = TailcatTransport::new(cli, TailcatAddress::parse(SAMPLE).unwrap());
            let (fwd, _lines) = t.forward(req()).await.unwrap();
            assert_eq!(fwd.local_addr(), "127.0.0.1:54321".parse().unwrap());
            fwd.shutdown().await.unwrap();
        }

        #[tokio::test]
        async fn bind_failure_keeps_tailcat_error() {
            let body = r#"[ "$1" = version ] && { echo v0.7.0; exit 0; }
echo "listen on 127.0.0.1:5432: bind: address already in use" >&2; exit 1"#;
            let cli = TailcatCli::locate(Some(&fake_bin("tailcat", body))).unwrap();
            let t = TailcatTransport::new(cli, TailcatAddress::parse(SAMPLE).unwrap());
            let err = t.forward(req()).await.unwrap_err().to_string();
            assert!(err.contains("address already in use"), "{err}");
        }

        #[tokio::test]
        async fn local_transport_does_not_forward() {
            let t = crate::transport::LocalTransport::new(ProcessSpec::new("/bin/cat"));
            let err = t.forward(req()).await.unwrap_err();
            assert!(
                matches!(err, TransportError::ForwardUnsupported(_)),
                "{err}"
            );
        }
    }
}
