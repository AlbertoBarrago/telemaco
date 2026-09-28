//! The agent: serves the remote protocol on one byte stream.
//!
//! Under Tailcat, `tailcat serve -- telemaco remote agent` starts one agent
//! process per connection with the connection as stdio, so an agent handles
//! exactly one session and exits. Stdout is the protocol channel: nothing
//! else may ever be written to it. Diagnostics go to stderr, which tailcat
//! forwards to the terminal running `remote serve`.

use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, WriteHalf};
use tokio::sync::mpsc;

use crate::protocol::{
    self, ErrorCode, ExecEnd, ExecExit, ExecRequest, OutputStream, ProtocolError, RemoteError,
    Request, Response, StatusInfo, PROTOCOL_VERSION,
};

/// Output is forwarded in chunks of at most this many bytes (about 43 KiB
/// once base64 encoded, far below the frame cap).
const OUTPUT_CHUNK: usize = 32 * 1024;
/// After the program exits, how long to keep collecting output still in the
/// pipes. Bounded because a background grandchild can hold them open forever.
const DRAIN_AFTER_EXIT: Duration = Duration::from_secs(1);

/// What this agent is and what it permits. Fixed for the agent's lifetime:
/// nothing a client sends can change it.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub telemaco_version: String,
    /// Whether `exec` requests are honored. Off unless the operator started
    /// `remote serve --allow-exec`.
    pub allow_exec: bool,
    /// When set, one audit line per exec (program name only, never the
    /// arguments, which may carry secrets) goes to stderr, tagged with this
    /// peer label. Stderr is the operator's log under `tailcat serve`.
    pub audit_peer: Option<String>,
}

/// One frame (or the end of the stream) as read by the session's reader.
type Incoming = Result<Option<Vec<u8>>, ProtocolError>;

/// Serves one session to completion. Returns `Ok(())` when the client
/// disconnects cleanly after the handshake.
///
/// A single reader future pulls frames off the stream for the whole session
/// and hands them over a channel. Reading a frame directly inside a
/// `select!` would not be cancel-safe: losing the race to a chunk of exec
/// output mid-frame would desynchronize the stream. `mpsc::Receiver::recv`
/// is cancel-safe, so the session can wait on it alongside a running exec.
pub async fn serve<S>(stream: &mut S, config: &AgentConfig) -> Result<(), ProtocolError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (mut rd, mut wr) = tokio::io::split(stream);
    let (tx, mut rx) = mpsc::channel::<Incoming>(1);
    let reader = async move {
        loop {
            let frame = protocol::read_frame(&mut rd).await;
            let last = !matches!(frame, Ok(Some(_)));
            if tx.send(frame).await.is_err() || last {
                break;
            }
        }
    };
    let session = async {
        handshake(&mut rx, &mut wr, config).await?;
        session_loop(&mut rx, &mut wr, config).await
    };
    tokio::pin!(session);
    // If the reader finishes first it has already queued EOF or an error;
    // the session still has to consume it, so keep awaiting the session.
    tokio::select! {
        result = &mut session => result,
        () = reader => session.await,
    }
}

type Writer<'a, S> = WriteHalf<&'a mut S>;

async fn next_frame(rx: &mut mpsc::Receiver<Incoming>) -> Incoming {
    rx.recv().await.unwrap_or(Ok(None))
}

async fn session_loop<S>(
    rx: &mut mpsc::Receiver<Incoming>,
    wr: &mut Writer<'_, S>,
    config: &AgentConfig,
) -> Result<(), ProtocolError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    loop {
        let body = match next_frame(rx).await {
            Ok(Some(body)) => body,
            Ok(None) => return Ok(()),
            // Framing is lost (or the peer is hostile): report and hang up.
            Err(e @ (ProtocolError::FrameTooLarge(_) | ProtocolError::EmptyFrame)) => {
                let _ = reply_error(wr, ErrorCode::BadRequest, e.to_string()).await;
                return Err(e);
            }
            Err(e) => return Err(e),
        };
        let request = match protocol::decode::<Request>(&body) {
            Ok(r) => r,
            // A well-framed but invalid message: say so and keep serving.
            Err(e) => {
                reply_error(wr, ErrorCode::BadRequest, e.to_string()).await?;
                continue;
            }
        };
        let response = match request {
            Request::Hello { .. } => Response::Error(RemoteError::new(
                ErrorCode::BadRequest,
                "handshake already done",
            )),
            Request::Ping { nonce } => Response::Pong { nonce },
            Request::Status {} => Response::Status(status(config).await),
            Request::Cancel {} => Response::Error(RemoteError::new(
                ErrorCode::BadRequest,
                "no exec is running",
            )),
            Request::Exec(req) => match exec(req, rx, wr, config).await? {
                ExecFlow::Continue => continue,
                ExecFlow::ClientGone => return Ok(()),
            },
        };
        protocol::send(wr, &response).await?;
    }
}

async fn handshake<S>(
    rx: &mut mpsc::Receiver<Incoming>,
    wr: &mut Writer<'_, S>,
    config: &AgentConfig,
) -> Result<(), ProtocolError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let first = next_frame(rx).await?.ok_or(ProtocolError::Closed)?;
    match protocol::decode::<Request>(&first) {
        Ok(Request::Hello { version }) if version == PROTOCOL_VERSION => {
            let hello = Response::Hello {
                version: PROTOCOL_VERSION,
                telemaco_version: config.telemaco_version.clone(),
            };
            protocol::send(wr, &hello).await
        }
        Ok(Request::Hello { version }) => {
            let msg = format!("agent speaks protocol v{PROTOCOL_VERSION}, client sent v{version}");
            reply_error(wr, ErrorCode::UnsupportedVersion, msg).await?;
            Err(ProtocolError::VersionMismatch {
                ours: PROTOCOL_VERSION,
                theirs: version,
            })
        }
        Ok(_) => {
            reply_error(wr, ErrorCode::BadRequest, "expected hello").await?;
            Err(ProtocolError::Unexpected("request before hello".into()))
        }
        Err(e) => {
            reply_error(wr, ErrorCode::BadRequest, e.to_string()).await?;
            Err(e)
        }
    }
}

async fn reply_error<W>(
    wr: &mut W,
    code: ErrorCode,
    message: impl Into<String>,
) -> Result<(), ProtocolError>
where
    W: AsyncWrite + Unpin,
{
    protocol::send(wr, &Response::Error(RemoteError::new(code, message))).await
}

enum ExecFlow {
    /// The exec is settled (ran, or was refused); keep serving the session.
    Continue,
    /// The client disconnected mid-exec; the program was killed.
    ClientGone,
}

/// Runs one program with no shell, streaming its output, until it exits, the
/// deadline passes, or the client cancels or disconnects. The child is
/// `kill_on_drop`, so every early return (including write errors to a vanished
/// client) also kills it.
///
/// Limitation: only the direct child is killed. Grandchildren it started in
/// the background keep running, as they would after an `ssh` session ends.
async fn exec<S>(
    req: ExecRequest,
    rx: &mut mpsc::Receiver<Incoming>,
    wr: &mut Writer<'_, S>,
    config: &AgentConfig,
) -> Result<ExecFlow, ProtocolError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    if !config.allow_exec {
        reply_error(
            wr,
            ErrorCode::Forbidden,
            "exec is disabled on this agent; the operator must start it with \
             `telemaco remote serve --allow-exec`",
        )
        .await?;
        return Ok(ExecFlow::Continue);
    }
    if let Err(e) = req.validate() {
        reply_error(wr, ErrorCode::BadRequest, e).await?;
        return Ok(ExecFlow::Continue);
    }
    let program = protocol::sanitize_for_display(&req.program);
    if let Some(peer) = &config.audit_peer {
        eprintln!(
            "telemaco agent: {peer} exec {program} ({} args)",
            req.args.len()
        );
    }

    let mut cmd = tokio::process::Command::new(&req.program);
    cmd.args(&req.args)
        .envs(req.env.iter().map(|v| (&v.name, &v.value)))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    if let Some(cwd) = &req.cwd {
        cmd.current_dir(cwd);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            reply_error(
                wr,
                ErrorCode::ExecFailed,
                format!("failed to start {program}: {e}"),
            )
            .await?;
            return Ok(ExecFlow::Continue);
        }
    };
    let (Some(mut out), Some(mut err)) = (child.stdout.take(), child.stderr.take()) else {
        reply_error(wr, ErrorCode::Internal, "child stdio was not piped").await?;
        return Ok(ExecFlow::Continue);
    };

    let deadline = async {
        match req.timeout_secs {
            Some(s) => tokio::time::sleep(Duration::from_secs(s)).await,
            None => std::future::pending().await,
        }
    };
    tokio::pin!(deadline);
    let mut out_buf = vec![0u8; OUTPUT_CHUNK];
    let mut err_buf = vec![0u8; OUTPUT_CHUNK];
    let (mut out_open, mut err_open) = (true, true);

    // Every branch here is cancel-safe: pipe reads, `recv`, `wait`, sleep.
    let (status, end) = loop {
        tokio::select! {
            r = out.read(&mut out_buf), if out_open => match r {
                Ok(0) | Err(_) => out_open = false,
                Ok(n) => protocol::send(wr, &Response::output(OutputStream::Stdout, &out_buf[..n])).await?,
            },
            r = err.read(&mut err_buf), if err_open => match r {
                Ok(0) | Err(_) => err_open = false,
                Ok(n) => protocol::send(wr, &Response::output(OutputStream::Stderr, &err_buf[..n])).await?,
            },
            status = child.wait() => break (status?, ExecEnd::Exited),
            () = &mut deadline => {
                child.kill().await?;
                break (child.wait().await?, ExecEnd::TimedOut);
            }
            frame = next_frame(rx) => {
                child.kill().await?;
                let status = child.wait().await?;
                match frame.map(|f| f.map(|b| protocol::decode::<Request>(&b))) {
                    Ok(Some(Ok(Request::Cancel {}))) => break (status, ExecEnd::Cancelled),
                    // Anything else mid-exec is a protocol violation: stop
                    // the program, say why, and end the exec as cancelled.
                    Ok(Some(_)) => {
                        reply_error(wr, ErrorCode::BadRequest, "only cancel is valid while an exec runs").await?;
                        break (status, ExecEnd::Cancelled);
                    }
                    Ok(None) | Err(_) => return Ok(ExecFlow::ClientGone),
                }
            }
        }
    };

    if end == ExecEnd::Exited {
        // Forward what is still buffered in the pipes, briefly.
        let drain = async {
            while out_open || err_open {
                tokio::select! {
                    r = out.read(&mut out_buf), if out_open => match r {
                        Ok(0) | Err(_) => out_open = false,
                        Ok(n) => protocol::send(wr, &Response::output(OutputStream::Stdout, &out_buf[..n])).await?,
                    },
                    r = err.read(&mut err_buf), if err_open => match r {
                        Ok(0) | Err(_) => err_open = false,
                        Ok(n) => protocol::send(wr, &Response::output(OutputStream::Stderr, &err_buf[..n])).await?,
                    },
                }
            }
            Ok::<_, ProtocolError>(())
        };
        if let Ok(result) = tokio::time::timeout(DRAIN_AFTER_EXIT, drain).await {
            result?;
        }
    }

    let exit = ExecExit {
        code: status.code(),
        signal: exit_signal(&status),
        end,
    };
    protocol::send(wr, &Response::Exited(exit)).await?;
    Ok(ExecFlow::Continue)
}

#[cfg(unix)]
fn exit_signal(status: &std::process::ExitStatus) -> Option<i32> {
    std::os::unix::process::ExitStatusExt::signal(status)
}

#[cfg(not(unix))]
fn exit_signal(_: &std::process::ExitStatus) -> Option<i32> {
    None
}

async fn status(config: &AgentConfig) -> StatusInfo {
    StatusInfo {
        hostname: hostname().await,
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        telemaco_version: config.telemaco_version.clone(),
        protocol_version: PROTOCOL_VERSION,
        exec_enabled: config.allow_exec,
    }
}

/// The machine's host name via the `hostname` utility (present on Linux,
/// macOS and Windows), which avoids an FFI call and a new dependency.
/// Any failure just means "unknown".
async fn hostname() -> Option<String> {
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        tokio::process::Command::new("hostname")
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !out.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!name.is_empty() && name.len() <= 255).then_some(name)
}

/// Short, non-secret label for the peer from Tailcat's `$TAILCAT_PEER_KEY`
/// (a public node key), for the operator's log. Truncated to keep log lines
/// readable; the full key is not needed to recognize a peer.
pub fn peer_label(raw: Option<&str>) -> String {
    match raw {
        Some(key) if !key.is_empty() => {
            let visible: String = key.chars().take(16).collect();
            format!("{}...", protocol::sanitize_for_display(&visible))
        }
        _ => "unknown peer".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{recv, send, write_frame};

    fn config() -> AgentConfig {
        AgentConfig {
            telemaco_version: "9.9.9".into(),
            allow_exec: false,
            audit_peer: None,
        }
    }

    /// Runs an agent on one end of an in-memory pipe and returns the other.
    fn spawn_agent() -> (
        tokio::io::DuplexStream,
        tokio::task::JoinHandle<Result<(), ProtocolError>>,
    ) {
        let (client, mut server) = tokio::io::duplex(64 * 1024);
        let handle = tokio::spawn(async move { serve(&mut server, &config()).await });
        (client, handle)
    }

    #[tokio::test]
    async fn handshake_ping_status_then_clean_close() {
        let (mut c, agent) = spawn_agent();
        send(&mut c, &Request::Hello { version: 1 }).await.unwrap();
        assert_eq!(
            recv::<_, Response>(&mut c).await.unwrap(),
            Response::Hello {
                version: 1,
                telemaco_version: "9.9.9".into()
            }
        );
        send(&mut c, &Request::Ping { nonce: 42 }).await.unwrap();
        assert_eq!(
            recv::<_, Response>(&mut c).await.unwrap(),
            Response::Pong { nonce: 42 }
        );
        send(&mut c, &Request::Status {}).await.unwrap();
        let Response::Status(s) = recv::<_, Response>(&mut c).await.unwrap() else {
            panic!()
        };
        assert_eq!(s.protocol_version, 1);
        assert_eq!(s.os, std::env::consts::OS);
        assert!(!s.exec_enabled);
        drop(c);
        agent.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn wrong_version_is_refused() {
        let (mut c, agent) = spawn_agent();
        send(&mut c, &Request::Hello { version: 2 }).await.unwrap();
        let Response::Error(e) = recv::<_, Response>(&mut c).await.unwrap() else {
            panic!()
        };
        assert_eq!(e.code, ErrorCode::UnsupportedVersion);
        assert!(matches!(
            agent.await.unwrap(),
            Err(ProtocolError::VersionMismatch { .. })
        ));
    }

    #[tokio::test]
    async fn requests_before_hello_are_refused() {
        let (mut c, agent) = spawn_agent();
        send(&mut c, &Request::Status {}).await.unwrap();
        let Response::Error(e) = recv::<_, Response>(&mut c).await.unwrap() else {
            panic!()
        };
        assert_eq!(e.code, ErrorCode::BadRequest);
        assert!(agent.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn malformed_request_gets_an_error_and_the_session_survives() {
        let (mut c, agent) = spawn_agent();
        send(&mut c, &Request::Hello { version: 1 }).await.unwrap();
        recv::<_, Response>(&mut c).await.unwrap();
        write_frame(&mut c, br#"{"type":"shell","cmd":"id"}"#)
            .await
            .unwrap();
        let Response::Error(e) = recv::<_, Response>(&mut c).await.unwrap() else {
            panic!()
        };
        assert_eq!(e.code, ErrorCode::BadRequest);
        send(&mut c, &Request::Ping { nonce: 1 }).await.unwrap();
        assert_eq!(
            recv::<_, Response>(&mut c).await.unwrap(),
            Response::Pong { nonce: 1 }
        );
        drop(c);
        agent.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn oversized_frame_ends_the_session() {
        use tokio::io::AsyncWriteExt;
        let (mut c, agent) = spawn_agent();
        send(&mut c, &Request::Hello { version: 1 }).await.unwrap();
        recv::<_, Response>(&mut c).await.unwrap();
        c.write_all(&u32::MAX.to_be_bytes()).await.unwrap();
        let Response::Error(e) = recv::<_, Response>(&mut c).await.unwrap() else {
            panic!()
        };
        assert_eq!(e.code, ErrorCode::BadRequest);
        assert!(matches!(
            agent.await.unwrap(),
            Err(ProtocolError::FrameTooLarge(_))
        ));
    }

    #[test]
    fn peer_label_truncates_and_sanitizes() {
        assert_eq!(peer_label(None), "unknown peer");
        assert_eq!(
            peer_label(Some("nodekey:cfb6bfa77a0654d7450947fd6acef17d")),
            "nodekey:cfb6bfa7..."
        );
        assert_eq!(peer_label(Some("\x1b[31mred")), "?[31mred...");
    }

    /// Exec end to end: real child processes, agent and client over an
    /// in-memory pipe.
    #[cfg(unix)]
    mod exec {
        use super::*;
        use crate::client::RemoteClient;
        use crate::protocol::{EnvVar, ErrorCode, ExecRequest};
        use std::time::Duration;

        fn req(program: &str, args: &[&str]) -> ExecRequest {
            ExecRequest {
                program: program.into(),
                args: args.iter().map(|a| a.to_string()).collect(),
                env: vec![],
                cwd: None,
                timeout_secs: None,
            }
        }

        /// Starts an agent with exec allowed; the returned handle resolves
        /// when its session ends.
        fn agent(
            allow_exec: bool,
        ) -> (
            tokio::io::DuplexStream,
            tokio::task::JoinHandle<Result<(), ProtocolError>>,
        ) {
            let (client, mut server) = tokio::io::duplex(256 * 1024);
            let cfg = AgentConfig {
                telemaco_version: "t".into(),
                allow_exec,
                audit_peer: None,
            };
            (
                client,
                tokio::spawn(async move { serve(&mut server, &cfg).await }),
            )
        }

        #[tokio::test]
        async fn arguments_arrive_verbatim_and_no_shell_is_involved() {
            let (mut c, _a) = agent(true);
            let mut client = RemoteClient::handshake(&mut c).await.unwrap();
            let r = client
                .exec_collect(
                    req(
                        "printf",
                        &["[%s]", "two words", "$(id)", "; exit 3", "", "*"],
                    ),
                    1 << 20,
                )
                .await
                .unwrap();
            assert_eq!(r.exit.code, Some(0));
            assert_eq!(r.exit.end, ExecEnd::Exited);
            assert_eq!(r.stdout, b"[two words][$(id)][; exit 3][][*]");
            assert!(r.stderr.is_empty());
        }

        #[tokio::test]
        async fn exit_code_stderr_env_and_cwd() {
            let (mut c, _a) = agent(true);
            let mut client = RemoteClient::handshake(&mut c).await.unwrap();
            let mut r = req("sh", &["-c", "echo \"$GREETING\" >&2; pwd; exit 7"]);
            r.env = vec![EnvVar {
                name: "GREETING".into(),
                value: "hi there".into(),
            }];
            r.cwd = Some("/".into());
            let r = client.exec_collect(r, 1 << 20).await.unwrap();
            assert_eq!(r.exit.code, Some(7));
            assert_eq!(r.stderr, b"hi there\n");
            assert_eq!(r.stdout, b"/\n");
            // The session is still usable after an exec.
            client.ping().await.unwrap();
        }

        #[tokio::test]
        async fn large_and_binary_output_is_streamed_intact() {
            let (mut c, _a) = agent(true);
            let mut client = RemoteClient::handshake(&mut c).await.unwrap();
            // 1 MiB of every byte value, more than one frame's worth.
            let r = client
                .exec_collect(
                    req("sh", &["-c", "i=0; while [ $i -lt 4096 ]; do printf '\\000\\377abc'; i=$((i+1)); done"]),
                    1 << 22,
                )
                .await
                .unwrap();
            assert_eq!(r.exit.code, Some(0));
            assert_eq!(r.stdout.len(), 4096 * 5);
            assert!(r.stdout.chunks(5).all(|c| c == b"\x00\xffabc"));
        }

        #[tokio::test]
        async fn collect_cap_truncates_but_the_program_finishes() {
            let (mut c, _a) = agent(true);
            let mut client = RemoteClient::handshake(&mut c).await.unwrap();
            let r = client
                .exec_collect(req("sh", &["-c", "printf 0123456789; exit 2"]), 4)
                .await
                .unwrap();
            assert_eq!(r.stdout, b"0123");
            assert!(r.truncated);
            assert_eq!(r.exit.code, Some(2));
        }

        #[tokio::test]
        async fn exec_is_forbidden_unless_allowed() {
            let (mut c, _a) = agent(false);
            let mut client = RemoteClient::handshake(&mut c).await.unwrap();
            let err = client.exec_collect(req("id", &[]), 1024).await.unwrap_err();
            let ProtocolError::Remote(e) = err else {
                panic!("{err}")
            };
            assert_eq!(e.code, ErrorCode::Forbidden);
            assert!(e.message.contains("--allow-exec"));
            client.ping().await.unwrap();
        }

        #[tokio::test]
        async fn missing_program_and_invalid_requests_are_errors() {
            let (mut c, _a) = agent(true);
            let mut client = RemoteClient::handshake(&mut c).await.unwrap();
            let err = client
                .exec_collect(req("/nonexistent/program", &[]), 1024)
                .await
                .unwrap_err();
            let ProtocolError::Remote(e) = err else {
                panic!("{err}")
            };
            assert_eq!(e.code, ErrorCode::ExecFailed);
            let mut bad = req("ls", &[]);
            bad.cwd = Some("relative".into());
            let ProtocolError::Remote(e) = client.exec_collect(bad, 1024).await.unwrap_err() else {
                panic!()
            };
            assert_eq!(e.code, ErrorCode::BadRequest);
            client.ping().await.unwrap();
        }

        #[tokio::test]
        async fn timeout_kills_the_program() {
            let (mut c, _a) = agent(true);
            let mut client = RemoteClient::handshake(&mut c).await.unwrap();
            let mut r = req("sleep", &["30"]);
            r.timeout_secs = Some(1);
            let start = std::time::Instant::now();
            let r = client.exec_collect(r, 1024).await.unwrap();
            assert_eq!(r.exit.end, ExecEnd::TimedOut);
            assert_eq!(r.exit.code, None);
            assert_eq!(r.exit.signal, Some(9));
            assert!(start.elapsed() < Duration::from_secs(10));
        }

        #[tokio::test]
        async fn cancel_kills_the_program_and_keeps_the_session() {
            let (mut c, _a) = agent(true);
            let mut client = RemoteClient::handshake(&mut c).await.unwrap();
            let mut seen = Vec::new();
            let cancel = tokio::time::sleep(Duration::from_millis(300));
            let exit = client
                .exec(
                    req("sh", &["-c", "echo started; exec sleep 30"]),
                    cancel,
                    Duration::from_secs(5),
                    |_, chunk| seen.extend_from_slice(chunk),
                )
                .await
                .unwrap();
            assert_eq!(exit.end, ExecEnd::Cancelled);
            assert_eq!(seen, b"started\n");
            client.ping().await.unwrap();
        }

        #[tokio::test]
        async fn client_disconnect_kills_the_program() {
            let (mut c, agent) = agent(true);
            let pidfile = std::env::temp_dir()
                .join(format!("telemaco-remote-exec-{}.pid", std::process::id()));
            {
                let mut client = RemoteClient::handshake(&mut c).await.unwrap();
                let script = format!("echo $$ > '{}'; exec sleep 30", pidfile.display());
                let run = client.exec(
                    req("sh", &["-c", &script]),
                    std::future::pending(),
                    Duration::ZERO,
                    |_, _| {},
                );
                // Give the program time to start, then walk away mid-exec.
                let _ = tokio::time::timeout(Duration::from_millis(500), run).await;
            }
            drop(c);
            // The agent must notice the disconnect promptly, not when the
            // program would have finished on its own.
            tokio::time::timeout(Duration::from_secs(5), agent)
                .await
                .expect("agent did not react to the client disconnecting")
                .unwrap()
                .unwrap();
            let pid = std::fs::read_to_string(&pidfile)
                .unwrap()
                .trim()
                .to_string();
            let alive = std::process::Command::new("kill")
                .args(["-0", &pid])
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success();
            assert!(!alive, "program {pid} outlived its client");
        }
    }
}
