//! `telemaco remote`: manage another machine over a remote transport.
//!
//! This module only parses arguments and prints. The protocol, the agent and
//! the transports (Tailcat, local) live in `telemaco-remote`; nothing here
//! branches on which transport is in use.

use std::io::Write as _;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use clap::Subcommand;
use telemaco_remote::agent::{self, AgentConfig};
use telemaco_remote::protocol::{
    sanitize_for_display, EnvVar, ErrorCode, ExecEnd, ExecRequest, OutputStream, ProtocolError,
};
use telemaco_remote::transport::{
    validate_forward_port, ForwardRequest, KeyName, NodeKey, ProcessSpec, ServeKey, ServeOptions,
    TailcatCli, TailcatServer,
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
/// How long the agent gets to confirm a cancelled exec before the transport
/// is torn down (which makes the agent kill the program anyway).
const CANCEL_GRACE: Duration = Duration::from_secs(5);

/// `remote exec` exit statuses for outcomes that are not the program's own,
/// following `ssh` (255: the connection failed) and `timeout(1)` (124).
const EXIT_TRANSPORT_FAILED: i32 = 255;
/// The remote program could not be started (the shell's "command not found").
const EXIT_NOT_STARTED: i32 = 127;
const EXIT_TIMED_OUT: i32 = 124;
const EXIT_CANCELLED: i32 = 130;

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

        /// Let clients run programs as the user running this server
        /// (`remote exec`). Off by default. Refused together with a saved
        /// --key unless --allow restricts who can connect.
        #[arg(long)]
        allow_exec: bool,

        /// Expose this local port (on localhost only) to `remote forward`
        /// clients. Repeatable. Nothing but the agent is reachable otherwise.
        #[arg(long = "forward-port", value_name = "PORT")]
        forward_ports: Vec<u16>,
    },

    /// Make a port of the remote machine available locally, until Ctrl-C.
    ///
    /// The remote side must have been started with `--forward-port PORT`.
    /// Listens on 127.0.0.1 unless --bind says otherwise.
    Forward {
        /// A tailcat address (tc...).
        target: String,

        /// Port on the remote machine's localhost.
        #[arg(value_parser = clap::value_parser!(u16).range(1..))]
        remote_port: u16,

        /// Local port to listen on; defaults to the remote port. `0` lets
        /// the OS pick a free one.
        #[arg(long, value_name = "PORT")]
        local_port: Option<u16>,

        /// Local address to listen on. Anything other than loopback makes
        /// the forwarded port reachable by other machines on your network.
        #[arg(long, value_name = "IP", default_value = "127.0.0.1")]
        bind: std::net::IpAddr,
    },

    /// Run a program on the remote machine, streaming its output. Exits with
    /// the program's status; otherwise 127 if it could not be started, 124 on
    /// --timeout, 130 if cancelled (Ctrl-C), 255 if the transport failed.
    ///
    /// The program and its arguments are passed as a list, never through a
    /// shell: `-- sh -c '...'` if a shell is really wanted.
    Exec {
        /// A tailcat address (tc...) or `local`.
        target: String,

        /// Absolute working directory on the remote machine.
        #[arg(long, value_name = "DIR")]
        cwd: Option<String>,

        /// Set an environment variable for the program. Repeatable.
        #[arg(long = "env", value_name = "NAME=VALUE")]
        env: Vec<String>,

        /// Have the agent kill the program after this many seconds.
        #[arg(long, value_name = "SECS", value_parser = clap::value_parser!(u64).range(1..))]
        timeout: Option<u64>,

        /// The program and its arguments, after `--`.
        #[arg(last = true, required = true, value_name = "PROGRAM")]
        command: Vec<String>,
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
    Agent {
        /// Honor exec requests. Set by `remote serve --allow-exec`.
        #[arg(long)]
        allow_exec: bool,

        /// A port `remote serve` forwards, reported in status. Repeatable.
        #[arg(long = "forwarded-port", value_name = "PORT")]
        forwarded_ports: Vec<u16>,
    },
}

/// Runs a remote subcommand and returns the process exit status.
pub async fn run(command: RemoteCommand) -> Result<i32> {
    match command {
        RemoteCommand::Serve {
            key,
            allow,
            allow_exec,
            forward_ports,
        } => serve(key, allow, allow_exec, forward_ports).await,
        RemoteCommand::Forward {
            target,
            remote_port,
            local_port,
            bind,
        } => {
            forward(
                &target,
                remote_port,
                local_port.unwrap_or(remote_port),
                bind,
            )
            .await
        }
        RemoteCommand::Status { target } => status(&target).await,
        RemoteCommand::Ping { target, count } => ping(&target, count).await,
        RemoteCommand::Exec {
            target,
            cwd,
            env,
            timeout,
            command,
        } => exec(&target, cwd, env, timeout, command).await,
        RemoteCommand::Agent {
            allow_exec,
            forwarded_ports,
        } => run_agent(allow_exec, forwarded_ports).await,
    }
    .map(|()| 0)
    .or_else(|e| match e.downcast::<ExecStatus>() {
        Ok(status) => Ok(status.0),
        Err(e) => Err(e),
    })
}

/// A non-zero exit status `remote exec` must propagate. Carried through
/// `anyhow` so every command keeps the same `Result<()>` shape.
#[derive(Debug)]
struct ExecStatus(i32);

impl std::fmt::Display for ExecStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "remote program exited with status {}", self.0)
    }
}

impl std::error::Error for ExecStatus {}

/// How the local transport and `tailcat serve` launch an agent: this very
/// executable, by absolute path (tailcat resolves bare names via PATH).
fn agent_spec(allow_exec: bool, forwarded_ports: &[u16]) -> Result<ProcessSpec> {
    let exe = std::env::current_exe().context("cannot locate the telemaco executable")?;
    let mut spec = ProcessSpec::new(exe).arg("remote").arg("agent");
    if allow_exec {
        spec = spec.arg("--allow-exec");
    }
    for port in forwarded_ports {
        spec = spec.arg("--forwarded-port").arg(port.to_string());
    }
    Ok(spec)
}

fn transport_for(target: &str) -> Result<AnyTransport> {
    let target = RemoteTarget::parse(target)?;
    let mut config = TransportConfig::from_env();
    // The local agent is this same user on this same machine with no network
    // in between, so exec crosses no trust boundary and is allowed.
    config.local_agent = Some(agent_spec(true, &[])?);
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
    if info.forwarded_ports.is_empty() {
        println!("Ports:     none forwarded");
    } else {
        let ports: Vec<String> = info.forwarded_ports.iter().map(u16::to_string).collect();
        println!("Ports:     {} (use `remote forward`)", ports.join(", "));
    }
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

/// Builds the wire request from CLI arguments, validating it locally so a
/// typo fails before any connection is made.
fn exec_request(
    cwd: Option<String>,
    env: Vec<String>,
    timeout: Option<u64>,
    command: Vec<String>,
) -> Result<ExecRequest> {
    let mut command = command.into_iter();
    let program = command.next().context("no program given after --")?;
    let env = env
        .iter()
        .map(|kv| {
            let (name, value) = kv
                .split_once('=')
                .with_context(|| format!("--env expects NAME=VALUE, got {kv:?}"))?;
            Ok(EnvVar {
                name: name.into(),
                value: value.into(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let request = ExecRequest {
        program,
        args: command.collect(),
        env,
        cwd,
        timeout_secs: timeout,
    };
    request
        .validate()
        .map_err(|e| anyhow!("invalid command: {e}"))?;
    Ok(request)
}

async fn exec(
    target: &str,
    cwd: Option<String>,
    env: Vec<String>,
    timeout: Option<u64>,
    command: Vec<String>,
) -> Result<()> {
    let request = exec_request(cwd, env, timeout, command)?;
    let status = match run_exec(target, request).await {
        Ok(status) => status,
        // Distinguish "could not run it" from anything the program returned.
        Err(e) => {
            eprintln!("Error: {e:?}");
            let not_started = matches!(
                e.downcast_ref::<ProtocolError>(),
                Some(ProtocolError::Remote(r)) if r.code == ErrorCode::ExecFailed
            );
            if not_started {
                EXIT_NOT_STARTED
            } else {
                EXIT_TRANSPORT_FAILED
            }
        }
    };
    if status == 0 {
        Ok(())
    } else {
        Err(ExecStatus(status).into())
    }
}

async fn run_exec(target: &str, request: ExecRequest) -> Result<i32> {
    let timeout = request.timeout_secs;
    let transport = transport_for(target)?;
    let mut conn = transport.connect().await?;

    // Ctrl-C, or local stdout going away (`| head`), cancels the remote
    // program rather than leaving it running.
    let local_closed = Arc::new(tokio::sync::Notify::new());
    let cancel = {
        let local_closed = Arc::clone(&local_closed);
        async move {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                () = local_closed.notified() => {}
            }
        }
    };
    let mut write_failed = false;
    let on_output = |stream: OutputStream, chunk: &[u8]| {
        let written = match stream {
            OutputStream::Stdout => {
                let mut out = std::io::stdout().lock();
                out.write_all(chunk).and_then(|()| out.flush())
            }
            OutputStream::Stderr => {
                let mut err = std::io::stderr().lock();
                err.write_all(chunk).and_then(|()| err.flush())
            }
        };
        if written.is_err() && !write_failed {
            write_failed = true;
            local_closed.notify_one();
        }
    };

    let outcome = async {
        let mut client = tokio::time::timeout(SESSION_TIMEOUT, RemoteClient::handshake(&mut conn))
            .await
            .map_err(|_| {
                ProtocolError::Unexpected(format!(
                    "timed out after {}s waiting for the remote agent",
                    SESSION_TIMEOUT.as_secs()
                ))
            })??;
        client.exec(request, cancel, CANCEL_GRACE, on_output).await
    }
    .await;
    // No overall deadline here: a remote build may legitimately run for
    // hours. `finish` only needs the timeout shape, so wrap as never-elapsed.
    let exit = finish(conn, Ok(outcome)).await?;

    Ok(match exit.end {
        ExecEnd::Exited => match (exit.code, exit.signal) {
            (Some(code), _) => code,
            (None, Some(signal)) => 128 + signal,
            (None, None) => EXIT_TRANSPORT_FAILED,
        },
        ExecEnd::TimedOut => {
            eprintln!(
                "telemaco: remote program killed after --timeout {}s",
                timeout.unwrap_or_default()
            );
            EXIT_TIMED_OUT
        }
        ExecEnd::Cancelled => EXIT_CANCELLED,
    })
}

async fn forward(
    target: &str,
    remote_port: u16,
    local_port: u16,
    bind: std::net::IpAddr,
) -> Result<()> {
    let transport = transport_for(target)?;

    // Ask the agent first: a port the server does not forward would only
    // fail later, per connection, with a far less obvious error.
    let mut conn = transport.connect().await?;
    let outcome = tokio::time::timeout(SESSION_TIMEOUT, async {
        let mut client = RemoteClient::handshake(&mut conn).await?;
        client.status().await
    })
    .await;
    let info = finish(conn, outcome).await?;
    if !info.forwarded_ports.contains(&remote_port) {
        bail!(
            "the remote does not forward port {remote_port}; restart it with \
             `telemaco remote serve --forward-port {remote_port}`"
        );
    }

    let request = ForwardRequest {
        remote_port,
        local_port,
        bind,
    };
    let (mut forwarder, mut lines) = transport.forward(request).await?;
    let local = forwarder.local_addr();
    if !bind.is_loopback() {
        eprintln!(
            "Warning: listening on {bind}, so other machines that can reach this one can use \
             the forwarded port."
        );
    }
    eprintln!(
        "Forwarding {local} -> remote localhost:{remote_port} over {}.",
        transport.kind()
    );
    // The local address alone on stdout, so scripts can capture it.
    println!("{local}");
    eprintln!("Press Ctrl-C to stop.");

    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);
    let outcome = loop {
        tokio::select! {
            () = &mut shutdown => break Ok(()),
            err = forwarder.wait() => break Err(anyhow!(err).context("forwarding stopped")),
            Some(line) = lines.recv() => eprintln!("{line}"),
        }
    };
    forwarder.shutdown().await?;
    outcome
}

async fn serve(
    key: Option<String>,
    allow: Vec<String>,
    allow_exec: bool,
    mut forward_ports: Vec<u16>,
) -> Result<()> {
    for port in &forward_ports {
        validate_forward_port(*port).map_err(|e| anyhow!("--forward-port: {e}"))?;
    }
    forward_ports.sort_unstable();
    forward_ports.dedup();
    let key = match key {
        None => ServeKey::Ephemeral,
        Some(name) => ServeKey::Saved(KeyName::parse(&name).map_err(|e| anyhow!("--key: {e}"))?),
    };
    let allow = allow
        .iter()
        .map(|k| NodeKey::parse(k).map_err(|e| anyhow!("--allow: {e}")))
        .collect::<Result<Vec<_>>>()?;
    // A stable address that anyone ever given it can reuse, fronting a
    // command runner, is a permanent unauthenticated shell. Never implicitly.
    if allow_exec && matches!(key, ServeKey::Saved(_)) && allow.is_empty() {
        bail!(
            "--allow-exec with a saved --key needs --allow: otherwise everyone who ever had \
             this address could run commands here in any future run"
        );
    }

    let cli = TailcatCli::locate(TransportConfig::from_env().tailcat_bin.as_deref())?;
    cli.ensure_supported().await?;
    let opts = ServeOptions {
        key,
        allow,
        agent: agent_spec(allow_exec, &forward_ports)?,
        forward_ports: forward_ports.clone(),
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
    if allow_exec {
        let user = std::env::var("USER").unwrap_or_else(|_| "the current user".into());
        eprintln!(
            "  exec:  ENABLED, clients can run programs as {}",
            sanitize_for_display(&user)
        );
    } else {
        eprintln!("  exec:  disabled (enable with --allow-exec)");
    }
    if forward_ports.is_empty() {
        eprintln!("  ports: none forwarded (add with --forward-port)");
    } else {
        let ports: Vec<String> = forward_ports.iter().map(u16::to_string).collect();
        eprintln!(
            "  ports: {} on localhost, for `remote forward`",
            ports.join(", ")
        );
    }
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

    // Created once: recreating it per iteration would re-register the
    // signal handlers on every relayed log line.
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);
    let outcome = loop {
        tokio::select! {
            () = &mut shutdown => break Ok(()),
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
async fn run_agent(allow_exec: bool, forwarded_ports: Vec<u16>) -> Result<()> {
    let peer = agent::peer_label(std::env::var("TAILCAT_PEER_KEY").ok().as_deref());
    let config = AgentConfig {
        telemaco_version: env!("TELEMACO_BUILD_VERSION").to_string(),
        allow_exec,
        audit_peer: Some(peer.clone()),
        forwarded_ports,
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
