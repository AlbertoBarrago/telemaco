//! Client side of the remote protocol, over any byte stream.

use std::future::Future;
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncWrite};

use crate::protocol::{
    self, ExecExit, ExecRequest, OutputStream, ProtocolError, Request, Response, StatusInfo,
    PROTOCOL_VERSION,
};

/// Everything an exec produced, for callers that want it in memory rather
/// than streamed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteExecutionResult {
    pub exit: ExecExit,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// Output beyond the caller's cap was dropped (the program still ran to
    /// completion).
    pub truncated: bool,
}

/// A handshaken session. Borrows the stream so the caller keeps the
/// transport connection and can ask it why the stream died.
pub struct RemoteClient<'a, S> {
    stream: &'a mut S,
    remote_version: String,
    next_nonce: u64,
}

impl<'a, S: AsyncRead + AsyncWrite + Unpin> RemoteClient<'a, S> {
    /// Sends `hello` and checks the agent speaks the same protocol version.
    pub async fn handshake(stream: &'a mut S) -> Result<Self, ProtocolError> {
        protocol::send(
            stream,
            &Request::Hello {
                version: PROTOCOL_VERSION,
            },
        )
        .await?;
        match protocol::recv(stream).await? {
            Response::Hello {
                version,
                telemaco_version,
            } if version == PROTOCOL_VERSION => Ok(Self {
                stream,
                remote_version: telemaco_version,
                next_nonce: 1,
            }),
            Response::Hello { version, .. } => Err(ProtocolError::VersionMismatch {
                ours: PROTOCOL_VERSION,
                theirs: version,
            }),
            Response::Error(e) => Err(e.into()),
            other => Err(unexpected(&other)),
        }
    }

    /// Telemaco version the agent reported in its hello.
    pub fn remote_version(&self) -> &str {
        &self.remote_version
    }

    /// Application-level round trip through the whole transport.
    pub async fn ping(&mut self) -> Result<Duration, ProtocolError> {
        let nonce = self.next_nonce;
        self.next_nonce += 1;
        let start = Instant::now();
        match self.call(&Request::Ping { nonce }).await? {
            Response::Pong { nonce: n } if n == nonce => Ok(start.elapsed()),
            other => Err(unexpected(&other)),
        }
    }

    pub async fn status(&mut self) -> Result<StatusInfo, ProtocolError> {
        match self.call(&Request::Status {}).await? {
            Response::Status(s) => Ok(s),
            other => Err(unexpected(&other)),
        }
    }

    /// Runs a program on the agent, handing each output chunk to
    /// `on_output` as it arrives. When `cancel` resolves, the agent is asked
    /// to kill the program; if it does not confirm within `cancel_grace`,
    /// this returns an error and the caller should drop the transport, which
    /// makes the agent kill the program anyway.
    pub async fn exec<F, C>(
        &mut self,
        request: ExecRequest,
        cancel: C,
        cancel_grace: Duration,
        mut on_output: F,
    ) -> Result<ExecExit, ProtocolError>
    where
        F: FnMut(OutputStream, &[u8]),
        C: Future<Output = ()>,
    {
        let (mut rd, mut wr) = tokio::io::split(&mut *self.stream);
        protocol::send(&mut wr, &Request::Exec(request)).await?;
        // One persistent reader future: never recreated inside the select,
        // so a partially read frame is never dropped.
        let reader = async {
            loop {
                match protocol::recv(&mut rd).await? {
                    Response::Output { stream, data } => {
                        on_output(stream, &protocol::decode_output(&data)?)
                    }
                    Response::Exited(exit) => return Ok(exit),
                    Response::Error(e) => return Err(e.into()),
                    other => return Err(unexpected(&other)),
                }
            }
        };
        tokio::pin!(reader);
        tokio::pin!(cancel);
        // Disarmed until a cancel is sent; the branch is gated on `cancelled`.
        let grace = tokio::time::sleep(Duration::from_secs(365 * 24 * 3600));
        tokio::pin!(grace);
        let mut cancelled = false;
        loop {
            tokio::select! {
                result = &mut reader => return result,
                () = &mut cancel, if !cancelled => {
                    cancelled = true;
                    protocol::send(&mut wr, &Request::Cancel {}).await?;
                    grace.as_mut().reset(tokio::time::Instant::now() + cancel_grace);
                }
                () = &mut grace, if cancelled => {
                    return Err(ProtocolError::Unexpected(
                        "the agent did not confirm cancellation".into(),
                    ));
                }
            }
        }
    }

    /// [`RemoteClient::exec`] collecting output in memory, keeping at most
    /// `max_bytes` of stdout and of stderr each.
    pub async fn exec_collect(
        &mut self,
        request: ExecRequest,
        max_bytes: usize,
    ) -> Result<RemoteExecutionResult, ProtocolError> {
        let (mut stdout, mut stderr, mut truncated) = (Vec::new(), Vec::new(), false);
        let exit = self
            .exec(
                request,
                std::future::pending(),
                Duration::ZERO,
                |stream, chunk| {
                    let buf = match stream {
                        OutputStream::Stdout => &mut stdout,
                        OutputStream::Stderr => &mut stderr,
                    };
                    let room = max_bytes.saturating_sub(buf.len());
                    truncated |= chunk.len() > room;
                    buf.extend_from_slice(&chunk[..chunk.len().min(room)]);
                },
            )
            .await?;
        Ok(RemoteExecutionResult {
            exit,
            stdout,
            stderr,
            truncated,
        })
    }

    async fn call(&mut self, request: &Request) -> Result<Response, ProtocolError> {
        protocol::send(self.stream, request).await?;
        match protocol::recv(self.stream).await? {
            Response::Error(e) => Err(e.into()),
            r => Ok(r),
        }
    }
}

fn unexpected(r: &Response) -> ProtocolError {
    let kind = match r {
        Response::Hello { .. } => "hello",
        Response::Pong { .. } => "pong",
        Response::Status(_) => "status",
        Response::Output { .. } => "output",
        Response::Exited(_) => "exited",
        Response::Error(_) => "error",
    };
    ProtocolError::Unexpected(kind.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{self, AgentConfig};
    use crate::protocol::{recv, send, ErrorCode};

    #[tokio::test]
    async fn client_and_agent_interoperate() {
        let (mut c, mut s) = tokio::io::duplex(64 * 1024);
        let cfg = AgentConfig {
            telemaco_version: "1.2.3".into(),
            allow_exec: true,
            audit_peer: None,
            forwarded_ports: vec![],
        };
        let agent = tokio::spawn(async move { agent::serve(&mut s, &cfg).await });
        let mut client = RemoteClient::handshake(&mut c).await.unwrap();
        assert_eq!(client.remote_version(), "1.2.3");
        client.ping().await.unwrap();
        client.ping().await.unwrap();
        assert!(client.status().await.unwrap().exec_enabled);
        drop(client);
        drop(c);
        agent.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn mismatched_pong_is_rejected() {
        let (mut c, mut s) = tokio::io::duplex(4096);
        tokio::spawn(async move {
            let _: Request = recv(&mut s).await.unwrap();
            send(
                &mut s,
                &Response::Hello {
                    version: 1,
                    telemaco_version: "x".into(),
                },
            )
            .await
            .unwrap();
            let _: Request = recv(&mut s).await.unwrap();
            send(&mut s, &Response::Pong { nonce: 999 }).await.unwrap();
        });
        let mut client = RemoteClient::handshake(&mut c).await.unwrap();
        assert!(matches!(
            client.ping().await,
            Err(ProtocolError::Unexpected(_))
        ));
    }

    #[tokio::test]
    async fn remote_errors_surface_with_their_code() {
        let (mut c, mut s) = tokio::io::duplex(4096);
        tokio::spawn(async move {
            let _: Request = recv(&mut s).await.unwrap();
            let e = protocol::RemoteError::new(ErrorCode::UnsupportedVersion, "v9 only");
            send(&mut s, &Response::Error(e)).await.unwrap();
        });
        let Err(ProtocolError::Remote(e)) = RemoteClient::handshake(&mut c).await else {
            panic!()
        };
        assert_eq!(e.code, ErrorCode::UnsupportedVersion);
    }

    #[tokio::test]
    async fn agent_vanishing_is_a_disconnect() {
        let (mut c, s) = tokio::io::duplex(4096);
        drop(s);
        let Err(err) = RemoteClient::handshake(&mut c).await else {
            panic!()
        };
        assert!(err.is_disconnect(), "{err}");
    }
}
