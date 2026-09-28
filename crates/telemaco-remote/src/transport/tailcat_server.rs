//! Server side of the Tailcat transport: `remote serve`.
//!
//! Runs `tailcat serve --key=new --json exec -- <agent argv>`. Tailcat's
//! `exec` service starts the agent once per incoming connection with the
//! connection as its stdio, so the only thing reachable through the tunnel is
//! the Telemaco protocol: no local port is exposed, nothing on the LAN, and
//! no exit node. The address comes from the `{"listenAddr": ...}` line that
//! `--json` writes to stdout, not from scraping the decorated stderr.
//!
//! Tailcat's stderr (its own notices plus every agent's diagnostics) is
//! relayed line by line with any address-shaped token redacted, because
//! tailcat itself prints the full address there.

use std::collections::VecDeque;
use std::fmt;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};
use tokio::process::Child;
use tokio::sync::mpsc;

use super::tailcat::AGENT_PORT;
use super::{ProcessSpec, TailcatCli, TransportError};
use crate::address::{redact_tailcat_tokens, TailcatAddress};

/// Lines of tailcat stderr kept for the error report if it exits.
const STDERR_TAIL_LINES: usize = 50;
/// Longest single stderr line relayed; the rest of the line is dropped.
const MAX_LINE_BYTES: u64 = 8 * 1024;

/// Which WireGuard identity the server uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServeKey {
    /// Fresh key for this run only: the address dies with the process. The
    /// default, and passed explicitly as `--key=new`, because plain
    /// `tailcat serve` silently reuses a saved `default` key if one exists.
    Ephemeral,
    /// A key saved with `tailcat genkey --key=<name>`: stable address, so
    /// anyone ever given it can reconnect in future runs.
    Saved(KeyName),
}

/// A saved-key name. Only plain names are accepted (tailcat also takes
/// paths), so the flag cannot point tailcat at an arbitrary file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyName(String);

impl KeyName {
    pub fn parse(s: &str) -> Result<Self, String> {
        let ok_chars = s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        if s.is_empty() || s.len() > 64 || !ok_chars || s.starts_with('.') {
            return Err(
                "key name must be 1-64 characters of [A-Za-z0-9._-], not starting with '.'".into(),
            );
        }
        if s == "new" {
            return Err("\"new\" means an ephemeral key, which is already the default".into());
        }
        Ok(Self(s.to_string()))
    }
}

impl fmt::Display for KeyName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A client public key for `--allow`, in tailcat's `nodekey:<64 hex>` form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeKey(String);

impl NodeKey {
    pub fn parse(s: &str) -> Result<Self, String> {
        let hex = s
            .strip_prefix("nodekey:")
            .ok_or("expected nodekey:<64 hex digits>")?;
        if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("expected nodekey:<64 hex digits>".into());
        }
        Ok(Self(s.to_ascii_lowercase()))
    }
}

#[derive(Debug, Clone)]
pub struct ServeOptions {
    pub key: ServeKey,
    /// Client node keys allowed through the tunnel. Empty means anyone who
    /// holds the address, which is then the credential.
    pub allow: Vec<NodeKey>,
    /// The agent command tailcat runs per connection. Must be an absolute
    /// path: tailcat resolves it with `LookPath` otherwise.
    pub agent: ProcessSpec,
    /// Local ports to expose through the tunnel (to `localhost` on this
    /// machine only). Empty by default: nothing but the agent is reachable.
    pub forward_ports: Vec<u16>,
}

/// Checks a port offered with `--forward-port`.
pub fn validate_forward_port(port: u16) -> Result<(), String> {
    match port {
        0 => Err("port 0 cannot be forwarded".into()),
        AGENT_PORT => Err(format!(
            "port {AGENT_PORT} is reserved for the Telemaco agent"
        )),
        _ => Ok(()),
    }
}

/// A running `tailcat serve`. Dropping it kills tailcat.
#[derive(Debug)]
pub struct TailcatServer {
    child: Child,
    address: TailcatAddress,
    spec: ProcessSpec,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
}

impl TailcatServer {
    /// The exact argv, exposed for tests and `--verbose` diagnostics.
    pub fn spec(cli: &TailcatCli, opts: &ServeOptions) -> ProcessSpec {
        let key = match &opts.key {
            ServeKey::Ephemeral => "--key=new".to_string(),
            ServeKey::Saved(name) => format!("--key={name}"),
        };
        let mut spec = cli.spec().arg("serve").arg(key).arg("--json");
        if !opts.allow.is_empty() {
            let keys: Vec<&str> = opts.allow.iter().map(|k| k.0.as_str()).collect();
            spec = spec.arg(format!("--allow={}", keys.join(",")));
        }
        // Positional ports and services are joined with commas by tailcat.
        // Listed ports proxy to localhost; every other port reaches `exec`.
        for port in &opts.forward_ports {
            spec = spec.arg(port.to_string());
        }
        spec = spec
            .arg("exec")
            .arg("--")
            .arg(opts.agent.program.as_os_str());
        spec.args.extend(opts.agent.args.iter().cloned());
        spec
    }

    /// Starts tailcat and waits (up to `startup`) for it to report its
    /// address. Returns the server and a channel of redacted stderr lines.
    pub async fn start(
        cli: &TailcatCli,
        opts: &ServeOptions,
        startup: Duration,
    ) -> Result<(Self, mpsc::Receiver<String>), TransportError> {
        let spec = Self::spec(cli, opts);
        let mut child = spec
            .command()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| spec.spawn_error(e))?;
        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            return Err(spec.spawn_error(std::io::Error::other("child stdio was not piped")));
        };

        let stderr_tail = Arc::new(Mutex::new(VecDeque::new()));
        let (tx, rx) = mpsc::channel(256);
        tokio::spawn(relay_stderr(stderr, tx, Arc::clone(&stderr_tail)));

        let mut stdout = BufReader::new(stdout);
        match tokio::time::timeout(startup, read_listen_addr(&mut stdout)).await {
            Ok(Ok(Some(address))) => {
                // Keep draining stdout so tailcat never blocks on it.
                tokio::spawn(async move {
                    let _ = tokio::io::copy(&mut stdout, &mut tokio::io::sink()).await;
                });
                Ok((
                    Self {
                        child,
                        address,
                        spec,
                        stderr_tail,
                    },
                    rx,
                ))
            }
            Ok(Ok(None)) | Ok(Err(_)) => Err(exit_report(&mut child, &spec, &stderr_tail).await),
            Err(_) => {
                let _ = child.kill().await;
                Err(TransportError::Timeout {
                    program: spec.display_name(),
                    seconds: startup.as_secs(),
                })
            }
        }
    }

    /// The address clients connect to. The caller decides where it goes;
    /// printing it once for the operator is the intended use.
    pub fn address(&self) -> &TailcatAddress {
        &self.address
    }

    /// Resolves when tailcat exits, with its status and recent stderr.
    pub async fn wait(&mut self) -> TransportError {
        exit_report(&mut self.child, &self.spec, &self.stderr_tail).await
    }

    /// Stops tailcat and reaps it. Running agents see their connection close.
    pub async fn shutdown(mut self) -> Result<(), TransportError> {
        match self.child.kill().await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::InvalidInput => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn id(&self) -> Option<u32> {
        self.child.id()
    }
}

/// Waits for tailcat to exit and describes how, with its recent stderr.
pub(crate) async fn exit_report(
    child: &mut Child,
    spec: &ProcessSpec,
    tail: &Mutex<VecDeque<String>>,
) -> TransportError {
    let status = match child.wait().await {
        Ok(s) => s,
        Err(e) => return e.into(),
    };
    // Let the relay task catch the last lines tailcat wrote before exiting.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let stderr = tail
        .lock()
        .map(|t| t.iter().cloned().collect::<Vec<_>>().join("\n"))
        .unwrap_or_default();
    TransportError::Exited {
        program: spec.display_name(),
        status: match status.code() {
            Some(c) => format!("status {c}"),
            None => status.to_string(),
        },
        stderr,
    }
}

#[derive(serde::Deserialize)]
struct ListenLine {
    #[serde(rename = "listenAddr")]
    listen_addr: String,
}

/// Reads stdout lines until the `--json` announcement. `Ok(None)` is EOF.
async fn read_listen_addr<R: AsyncRead + Unpin>(
    r: &mut BufReader<R>,
) -> std::io::Result<Option<TailcatAddress>> {
    loop {
        let mut line = Vec::new();
        if (&mut *r)
            .take(MAX_LINE_BYTES)
            .read_until(b'\n', &mut line)
            .await?
            == 0
        {
            return Ok(None);
        }
        let Ok(parsed) = serde_json::from_slice::<ListenLine>(&line) else {
            continue;
        };
        match TailcatAddress::parse(&parsed.listen_addr) {
            Ok(addr) => return Ok(Some(addr)),
            Err(e) => tracing::warn!("tailcat reported an unusable address: {e}"),
        }
    }
}

pub(crate) async fn relay_stderr<R: AsyncRead + Unpin>(
    r: R,
    tx: mpsc::Sender<String>,
    tail: Arc<Mutex<VecDeque<String>>>,
) {
    let mut r = BufReader::new(r);
    loop {
        let mut line = Vec::new();
        match (&mut r)
            .take(MAX_LINE_BYTES)
            .read_until(b'\n', &mut line)
            .await
        {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let text = redact_tailcat_tokens(String::from_utf8_lossy(&line).trim_end());
        if let Ok(mut t) = tail.lock() {
            if t.len() == STDERR_TAIL_LINES {
                t.pop_front();
            }
            t.push_back(text.clone());
        }
        // A slow or absent consumer must never stall tailcat: drop instead.
        let _ = tx.try_send(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "nodekey:cfb6bfa77a0654d7450947fd6acef17d2cd848da1d30b2540b13dac272ddfd16";

    #[test]
    fn key_names_are_plain_names_only() {
        assert!(KeyName::parse("default").is_ok());
        assert!(KeyName::parse("work-mac_2.v1").is_ok());
        for bad in ["", "new", "../etc/passwd", "/abs", ".hidden", "a b", "k=1"] {
            assert!(KeyName::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn node_keys_are_validated() {
        assert!(NodeKey::parse(KEY).is_ok());
        assert_eq!(
            NodeKey::parse(&KEY.to_uppercase().replace("NODEKEY", "nodekey"))
                .unwrap()
                .0,
            KEY
        );
        for bad in [
            "cfb6bf",
            "nodekey:xyz",
            "nodekey:",
            &format!("{KEY}00"),
            &format!("{KEY},nodekey:00"),
        ] {
            assert!(NodeKey::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[cfg(unix)]
    mod unix {
        use super::*;
        use crate::address::tests::SAMPLE;
        use crate::transport::test_support::fake_bin;

        fn opts() -> ServeOptions {
            ServeOptions {
                key: ServeKey::Ephemeral,
                allow: vec![],
                agent: ProcessSpec::new("/opt/telemaco").arg("remote").arg("agent"),
                forward_ports: vec![],
            }
        }

        #[test]
        fn forwarded_ports_go_before_the_exec_service() {
            let cli = TailcatCli::locate(Some(&fake_bin("tailcat", "exit 0"))).unwrap();
            let mut o = opts();
            o.forward_ports = vec![5432, 8080];
            let got = argv(&TailcatServer::spec(&cli, &o));
            assert_eq!(
                &got[..6],
                ["serve", "--key=new", "--json", "5432", "8080", "exec"]
            );
        }

        #[test]
        fn forward_port_validation() {
            assert!(validate_forward_port(5432).is_ok());
            assert!(validate_forward_port(0).is_err());
            assert!(validate_forward_port(AGENT_PORT).is_err());
        }

        fn argv(spec: &ProcessSpec) -> Vec<String> {
            spec.args
                .iter()
                .map(|a| match a {
                    crate::transport::Arg::Plain(s) => s.to_string_lossy().into_owned(),
                    crate::transport::Arg::Secret(_) => "<secret>".into(),
                })
                .collect()
        }

        #[test]
        fn default_argv_is_ephemeral_exec_only() {
            let cli = TailcatCli::locate(Some(&fake_bin("tailcat", "exit 0"))).unwrap();
            let got = argv(&TailcatServer::spec(&cli, &opts()));
            assert_eq!(
                got,
                [
                    "serve",
                    "--key=new",
                    "--json",
                    "exec",
                    "--",
                    "/opt/telemaco",
                    "remote",
                    "agent"
                ]
            );
        }

        #[test]
        fn saved_key_and_allow_list_are_passed_through() {
            let cli = TailcatCli::locate(Some(&fake_bin("tailcat", "exit 0"))).unwrap();
            let mut o = opts();
            o.key = ServeKey::Saved(KeyName::parse("home").unwrap());
            o.allow = vec![NodeKey::parse(KEY).unwrap(), NodeKey::parse(KEY).unwrap()];
            let got = argv(&TailcatServer::spec(&cli, &o));
            assert_eq!(got[1], "--key=home");
            assert_eq!(got[3], format!("--allow={KEY},{KEY}"));
            assert_eq!(got[4], "exec");
        }

        #[tokio::test]
        async fn startup_reads_the_json_address_and_redacts_stderr() {
            let body = format!(
                "echo '# Selected bootstrap relay region 302, San Francisco' >&2\n\
                 echo '# 🐈 Server listening with new address: {SAMPLE}' >&2\n\
                 echo '{{\"listenAddr\":\"{SAMPLE}\"}}'\n\
                 exec sleep 30"
            );
            let cli = TailcatCli::locate(Some(&fake_bin("tailcat", &body))).unwrap();
            let (server, mut lines) = TailcatServer::start(&cli, &opts(), Duration::from_secs(5))
                .await
                .unwrap();
            assert_eq!(server.address().expose(), SAMPLE);
            let first = lines.recv().await.unwrap();
            assert!(first.contains("bootstrap relay"));
            let second = lines.recv().await.unwrap();
            assert!(second.ends_with("tcomFw...****"), "{second}");
            assert!(!second.contains(SAMPLE));
            server.shutdown().await.unwrap();
        }

        #[tokio::test]
        async fn startup_failure_keeps_tailcat_error() {
            let body = "echo 'exec command: exec: \"telemaco\": executable file not found in $PATH' >&2; exit 1";
            let cli = TailcatCli::locate(Some(&fake_bin("tailcat", body))).unwrap();
            let err = TailcatServer::start(&cli, &opts(), Duration::from_secs(5))
                .await
                .unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("status 1"), "{msg}");
            assert!(msg.contains("executable file not found"), "{msg}");
        }

        #[tokio::test]
        async fn startup_timeout_kills_tailcat() {
            let cli = TailcatCli::locate(Some(&fake_bin("tailcat", "exec sleep 30"))).unwrap();
            let err = TailcatServer::start(&cli, &opts(), Duration::from_millis(300))
                .await
                .unwrap_err();
            assert!(matches!(err, TransportError::Timeout { .. }), "{err}");
        }

        #[tokio::test]
        async fn unexpected_exit_after_startup_is_reported() {
            let body = format!("echo '{{\"listenAddr\":\"{SAMPLE}\"}}'; sleep 0.2; echo 'derp: connection lost' >&2; exit 4");
            let cli = TailcatCli::locate(Some(&fake_bin("tailcat", &body))).unwrap();
            let (mut server, _lines) = TailcatServer::start(&cli, &opts(), Duration::from_secs(5))
                .await
                .unwrap();
            let msg = server.wait().await.to_string();
            assert!(msg.contains("status 4"), "{msg}");
            assert!(msg.contains("derp: connection lost"), "{msg}");
        }
    }
}
