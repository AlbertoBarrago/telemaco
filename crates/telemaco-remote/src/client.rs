//! Client side of the remote protocol, over any byte stream.

use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncWrite};

use crate::protocol::{self, ProtocolError, Request, Response, StatusInfo, PROTOCOL_VERSION};

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
