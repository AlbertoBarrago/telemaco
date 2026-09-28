# telemaco-remote

Remote protocol and pluggable transports for
[Telemaco](https://github.com/AlbertoBarrago/telemaco): status, ping,
structured exec and port forwarding to another machine, carried over
[Tailcat](https://github.com/tailscale/tailcat) or a local agent.

## Place in the workspace

A leaf layer with no dependency on V8 or the browser crates: it only moves
bytes between processes. `telemaco-cli` builds the `telemaco remote`
subcommands on top of it.

- `protocol`: versioned, length-prefixed JSON frames (v1), capped at 1 MiB,
  decoded into closed message enums that reject unknown fields.
- `agent`: serves one protocol session on any byte stream.
- `client`: `RemoteClient` (handshake, ping, status, streaming exec).
- `transport`: the `Transport` trait and its implementations. `tailcat`
  drives the external `tailcat` CLI as a child process (never through a
  shell); `local` runs the agent on this machine.

The protocol never knows which transport is underneath: a transport only has
to produce a `Connection`, an `AsyncRead + AsyncWrite` stream.

## Usage

```rust,no_run
use telemaco_remote::{select_transport, RemoteClient, RemoteTarget, Transport, TransportConfig};

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let target = RemoteTarget::parse("tcXXXXXXXXXXXXXXXXXXXXXXXX")?;
let transport = select_transport(target, &TransportConfig::from_env())?;
let mut conn = transport.connect().await?;
let mut client = RemoteClient::handshake(&mut conn).await?;
println!("{:?}", client.status().await?);
# Ok(())
# }
```

## Security

`TailcatAddress` is redacted in `Debug` and `Display`; the full value is only
reachable through `expose()`. See
[Remote over Tailcat](https://github.com/AlbertoBarrago/telemaco/blob/main/docs/Remote-over-Tailcat.md)
for the full model.

## Features

This crate has no cargo features.

Part of [Telemaco](https://github.com/AlbertoBarrago/telemaco).
