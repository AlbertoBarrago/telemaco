//! Local transport: the agent runs as a child process on this machine and
//! speaks the protocol over its stdio. Used for testing the protocol end to
//! end and as the reference transport any network transport must match.

use super::{Connection, ProcessSpec, Transport, TransportError, TransportKind};

#[derive(Debug, Clone)]
pub struct LocalTransport {
    agent: ProcessSpec,
}

impl LocalTransport {
    pub fn new(agent: ProcessSpec) -> Self {
        Self { agent }
    }
}

impl Transport for LocalTransport {
    fn kind(&self) -> TransportKind {
        TransportKind::Local
    }

    async fn connect(&self) -> Result<Connection, TransportError> {
        Connection::spawn(self.agent.clone())
    }
}
