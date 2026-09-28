//! Telemaco Remote: a small application protocol for managing another machine
//! (status, structured exec, port forwarding) and the transports that carry it.
//!
//! The protocol layer never knows which transport is underneath. A transport
//! only has to produce a [`transport::Connection`], a bidirectional byte stream
//! to a remote agent. Tailcat is one such transport: it is driven as an
//! external `tailcat` CLI process, never linked or reimplemented.

pub mod address;
pub mod agent;
pub mod client;
pub mod protocol;
pub mod transport;

pub use address::{AddressError, TailcatAddress};
pub use client::RemoteClient;
pub use transport::{
    select_transport, AnyTransport, Connection, PathInfo, RemoteTarget, Transport, TransportConfig,
    TransportError, TransportKind,
};
