//! `telemaco remote`: manage another machine over a remote transport.
//!
//! This module only parses arguments and prints. The protocol, the agent and
//! the transports (Tailcat, local) live in `telemaco-remote`; nothing here
//! branches on which transport is in use.

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use clap::Subcommand;
use telemaco_remote::agent::{self, AgentConfig};
use telemaco_remote::protocol::{sanitize_for_display, ProtocolError};
use telemaco_remote::transport::{
    KeyName, NodeKey, ProcessSpec, ServeKey, ServeOptions, TailcatCli, TailcatServer,
};
use telemaco_remote::{
    select_transport, AnyTransport, Connection, RemoteClient, RemoteTarget, Transport,
    TransportConfig,
};

/// Budget for reaching the agent. Covers tailcat's DERP bootstrap, the
/// WireGuard handshake and NAT traversal, which take seconds, not millis.
const SESSION_TIMEOUT: Duration = Duration::from_secs(45);
/// How long `remote serve` waits for tailcat to announce its address.
const SERVE_STARTUP_TIMEOUT: Duration = Duration::from_secs(60);
/// Grace period for a transport process to exit after EOF before it is killed.
const CLOSE_GRACE: Duration = Duration::from_secs(2);

#[derive(Subcommand, Debug)]
pub enum RemoteCommand {
    /// Serve the Telemaco remote agent over Tailcat and print its address.
    ///
    /// Only the Telemaco protocol is reachable through the tunnel: no local
    /// ports, no LAN, no exit node. The printed address is the credential
    /// unless --allow restricts clients.
    Serve {
        /// Use a key saved with `tailcat genkey --key=<NAME>` instead of a
        /// fresh one-run key. The address then survives restarts, so anyone
        /// ever given it can reconnect; pair it with --allow.
        #[arg(long, value_name = "NAME")]
        key: Option<String>,

        /// Only accept tunnel clients presenting this node key (from
        /// `tailcat genkey --client`). Repeatable.
        #[arg(long = "allow", value_name = "NODEKEY")]
        allow: Vec<String>,
    },

    /// Show the remote machine: host, OS, Telemaco version, network path.
    Status {
        /// A tailcat address (tc...) or `local`.
        target: String,
    },

    /// Measure round trips to the remote agent through the whole transport.
    Ping {
        /// A tailcat address (tc...) or `local`.
        target: String,

        /// Number of pings.
        #[arg(short = 'c', long, default_value_t = 4, value_parser = clap::value_parser!(u32).range(1..=100))]
        count: u32,
    },

    /// Speak the protocol on stdin/stdout. Started by `tailcat serve` (or the
    /// local transport) once per connection; not meant to be run by hand.
    #[command(hide = true)]
    Agent,
}

pub async fn run(command: RemoteCommand) -> Result<()> {
    match command {
        RemoteCommand::Serve { key, allow } => serve(key, allow).await,
        RemoteCommand::Status { target } => status(&target).await,
        RemoteCommand::Ping { target, count } => ping(&target, count).await,
        RemoteCommand::Agent => run_agent().await,
    }
}

/// How the local transport and `tailcat serve` launch an agent: this very
/// executable, by absolute path (tailcat resolves bare names via PATH).
fn agent_spec() -> Result<ProcessSpec> {
    let exe = std::env::current_exe().context("cannot locate the telemaco executable")?;
    Ok(ProcessSpec::new(exe).arg("remote").arg("agent"))
}

fn transport_for(target: &str) -> Result<AnyTransport> {
    let target = RemoteTarget::parse(target)?;
    let mut config = TransportConfig::from_env();
    config.local_agent = Some(agent_spec()?);
    Ok(select_transport(target, &config)?)
}

/// Settles a session: on success close politely; if the stream died, ask the
/// transport why (tailcat's own stderr); otherwise kill the child and report.
async fn finish<T>(
    conn: Connection,
    outcome: Result<Result<T, ProtocolError>, tokio::time::error::Elapsed>,
) -> Result<T> {
    match outcome {
        Ok(Ok(value)) => {
            if let Err(e) = conn.close(CLOSE_GRACE).await {
                tracing::debug!("transport exited uncleanly after the session: {e}");
            }
            Ok(value)
        }
        Ok(Err(e)) if e.is_disconnect() => {
            let why = conn.failure().await;
            Err(anyhow!(why).context("the remote agent could not be reached"))
        }
        Ok(Err(e)) => {
            let _ = conn.abort().await;
            Err(anyhow!(e))
        }
        Err(_) => {
            let _ = conn.abort().await;
            bail!(
                "timed out after {}s waiting for the remote agent",
                SESSION_TIMEOUT.as_secs()
            )
        }
    }
}

async fn status(target: &str) -> Result<()> {
    let transport = transport_for(target)?;
    let mut conn = transport.connect().await?;
    let session = tokio::time::timeout(SESSION_TIMEOUT, async {
        let mut client = RemoteClient::handshake(&mut conn).await?;
        client.status().await
    });
    // The path probe is independent of the session, so run both at once.
    let (outcome, path) = tokio::join!(session, transport.path());
    let info = finish(conn, outcome).await?;

    println!("Transport: {}", transport.kind());
    match path {
        Ok(Some(p)) => println!("Path:      {p}"),
        Ok(None) => {}
        Err(e) => println!("Path:      unknown ({e})"),
    }
    let host = info.hostname.as_deref().map(sanitize_for_display);
    println!("Remote:    {}", host.as_deref().unwrap_or("unknown"));
    println!(
        "OS:        {} {}",
        sanitize_for_display(&info.os),
        sanitize_for_display(&info.arch)
    );
    println!(
        "Telemaco:  {}",
        sanitize_for_display(&info.telemaco_version)
    );
    println!("Protocol:  v{}", info.protocol_version);
    println!(
        "Exec:      {}",
        if info.exec_enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    Ok(())
}

async fn ping(target: &str, count: u32) -> Result<()> {
    let transport = transport_for(target)?;
    let mut conn = transport.connect().await?;
    let outcome = tokio::time::timeout(SESSION_TIMEOUT, async {
        let mut client = RemoteClient::handshake(&mut conn).await?;
        let mut rtts = Vec::with_capacity(count as usize);
        for seq in 1..=count {
            let rtt = client.ping().await?;
            println!("pong {seq}: {:.1}ms", rtt.as_secs_f64() * 1000.0);
            rtts.push(rtt);
        }
        Ok(rtts)
    })
    .await;
    let rtts = finish(conn, outcome).await?;
    let ms = |d: &Duration| d.as_secs_f64() * 1000.0;
    let min = rtts.iter().map(ms).fold(f64::INFINITY, f64::min);
    let max = rtts.iter().map(ms).fold(0.0, f64::max);
    let avg = rtts.iter().map(ms).sum::<f64>() / rtts.len() as f64;
    println!(
        "{} pings over {}: min {min:.1}ms, avg {avg:.1}ms, max {max:.1}ms",
        rtts.len(),
        transport.kind()
    );
    Ok(())
}

async fn serve(key: Option<String>, allow: Vec<String>) -> Result<()> {
    let key = match key {
        None => ServeKey::Ephemeral,
        Some(name) => ServeKey::Saved(KeyName::parse(&name).map_err(|e| anyhow!("--key: {e}"))?),
    };
    let allow = allow
        .iter()
        .map(|k| NodeKey::parse(k).map_err(|e| anyhow!("--allow: {e}")))
        .collect::<Result<Vec<_>>>()?;

    let cli = TailcatCli::locate(TransportConfig::from_env().tailcat_bin.as_deref())?;
    cli.ensure_supported().await?;
    let opts = ServeOptions {
        key,
        allow,
        agent: agent_spec()?,
    };

    eprintln!("Starting Tailcat...");
    let (mut server, mut lines) = TailcatServer::start(&cli, &opts, SERVE_STARTUP_TIMEOUT).await?;

    eprintln!("Telemaco remote agent is reachable over Tailcat.");
    match &opts.key {
        ServeKey::Ephemeral => {
            eprintln!("  key:   ephemeral (the address stops working when this process exits)")
        }
        ServeKey::Saved(name) => {
            eprintln!("  key:   saved \"{name}\" (the address stays valid across restarts)")
        }
    }
    eprintln!("  exec:  disabled");
    if opts.allow.is_empty() {
        eprintln!("  allow: anyone who has the address");
        eprintln!("Share the address only over a private channel: it is the credential.");
        if matches!(opts.key, ServeKey::Saved(_)) {
            eprintln!(
                "Warning: with a saved key, everyone you ever shared this address with can \
                 reconnect. Consider --allow."
            );
        }
    } else {
        eprintln!("  allow: {} client key(s)", opts.allow.len());
    }
    // The address alone on stdout, so scripts can capture it.
    println!("{}", server.address().expose());
    eprintln!("Press Ctrl-C to stop.");

    let outcome = loop {
        tokio::select! {
            _ = shutdown_signal() => break Ok(()),
            err = server.wait() => break Err(anyhow!(err).context("tailcat stopped")),
            Some(line) = lines.recv() => eprintln!("{line}"),
        }
    };
    server.shutdown().await?;
    outcome
}

/// Ctrl-C, or SIGTERM on unix, so a supervisor stopping `remote serve` still
/// tears tailcat down instead of orphaning it.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// One protocol session on stdio. Stdout belongs to the protocol, so every
/// diagnostic goes to stderr, which `remote serve` shows the operator.
async fn run_agent() -> Result<()> {
    let peer = agent::peer_label(std::env::var("TAILCAT_PEER_KEY").ok().as_deref());
    let config = AgentConfig {
        telemaco_version: env!("TELEMACO_BUILD_VERSION").to_string(),
        allow_exec: false,
    };
    eprintln!("telemaco agent: session from {peer}");
    let mut stdio = tokio::io::join(tokio::io::stdin(), tokio::io::stdout());
    match agent::serve(&mut stdio, &config).await {
        Ok(()) => {
            eprintln!("telemaco agent: session from {peer} ended");
            Ok(())
        }
        Err(e) => {
            eprintln!("telemaco agent: session from {peer} failed: {e}");
            Err(anyhow!(e))
        }
    }
}
