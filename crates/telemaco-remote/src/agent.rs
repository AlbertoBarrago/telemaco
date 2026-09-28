//! The agent: serves the remote protocol on one byte stream.
//!
//! Under Tailcat, `tailcat serve -- telemaco remote agent` starts one agent
//! process per connection with the connection as stdio, so an agent handles
//! exactly one session and exits. Stdout is the protocol channel: nothing
//! else may ever be written to it. Diagnostics go to stderr, which tailcat
//! forwards to the terminal running `remote serve`.

use tokio::io::{AsyncRead, AsyncWrite};

use crate::protocol::{
    self, ErrorCode, ProtocolError, RemoteError, Request, Response, StatusInfo, PROTOCOL_VERSION,
};

/// What this agent is and what it permits. Fixed for the agent's lifetime:
/// nothing a client sends can change it.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub telemaco_version: String,
    /// Whether `exec` requests are honored. Off unless the operator started
    /// `remote serve --allow-exec`.
    pub allow_exec: bool,
}

/// Serves one session to completion. Returns `Ok(())` when the client
/// disconnects cleanly after the handshake.
pub async fn serve<S>(stream: &mut S, config: &AgentConfig) -> Result<(), ProtocolError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    handshake(stream, config).await?;
    loop {
        let body = match protocol::read_frame(stream).await {
            Ok(Some(body)) => body,
            Ok(None) => return Ok(()),
            // Framing is lost (or the peer is hostile): report and hang up.
            Err(e @ (ProtocolError::FrameTooLarge(_) | ProtocolError::EmptyFrame)) => {
                let _ = reply_error(stream, ErrorCode::BadRequest, e.to_string()).await;
                return Err(e);
            }
            Err(e) => return Err(e),
        };
        let request = match protocol::decode::<Request>(&body) {
            Ok(r) => r,
            // A well-framed but invalid message: say so and keep serving.
            Err(e) => {
                reply_error(stream, ErrorCode::BadRequest, e.to_string()).await?;
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
        };
        protocol::send(stream, &response).await?;
    }
}

async fn handshake<S>(stream: &mut S, config: &AgentConfig) -> Result<(), ProtocolError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let first = protocol::read_frame(stream)
        .await?
        .ok_or(ProtocolError::Closed)?;
    match protocol::decode::<Request>(&first) {
        Ok(Request::Hello { version }) if version == PROTOCOL_VERSION => {
            let hello = Response::Hello {
                version: PROTOCOL_VERSION,
                telemaco_version: config.telemaco_version.clone(),
            };
            protocol::send(stream, &hello).await
        }
        Ok(Request::Hello { version }) => {
            let msg = format!("agent speaks protocol v{PROTOCOL_VERSION}, client sent v{version}");
            reply_error(stream, ErrorCode::UnsupportedVersion, msg).await?;
            Err(ProtocolError::VersionMismatch {
                ours: PROTOCOL_VERSION,
                theirs: version,
            })
        }
        Ok(_) => {
            reply_error(stream, ErrorCode::BadRequest, "expected hello").await?;
            Err(ProtocolError::Unexpected("request before hello".into()))
        }
        Err(e) => {
            reply_error(stream, ErrorCode::BadRequest, e.to_string()).await?;
            Err(e)
        }
    }
}

async fn reply_error<S>(
    stream: &mut S,
    code: ErrorCode,
    message: impl Into<String>,
) -> Result<(), ProtocolError>
where
    S: AsyncWrite + Unpin,
{
    protocol::send(stream, &Response::Error(RemoteError::new(code, message))).await
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
}
