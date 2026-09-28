`telemaco remote` lets one machine check on, run programs on, and reach ports of another machine, with no public inbound port, no VPN to configure and no Tailscale account.

The work is split in two:

```
Telemaco Remote
    |
    +-- protocol (Telemaco)       status, ping, structured exec, forwarded ports
    |
    +-- Tailcat transport         the external `tailcat` CLI
            |
            +-- WireGuard encryption, end to end
            +-- NAT traversal, direct peer-to-peer when possible
            +-- DERP relay fallback when it is not
```

[Tailcat](https://github.com/tailscale/tailcat) moves the bytes. Telemaco provides the application protocol and decides what a client is allowed to do. Telemaco drives the `tailcat` binary as a separate process and never reimplements any of its networking.

## Install Tailcat

Tailcat is an optional runtime dependency: every other Telemaco command works without it.

```bash
brew install tailcat          # macOS
tailcat version               # Telemaco needs v0.7.0 or newer
```

See Tailcat's [INSTALL.md](https://github.com/tailscale/tailcat/blob/main/INSTALL.md) for Linux, Windows and other options. Telemaco looks for `tailcat` on `PATH`; set `TELEMACO_TAILCAT_BIN` to use a specific binary.

## Quick start

On the machine to manage:

```bash
telemaco remote serve
```

```
Starting Tailcat...
Telemaco remote agent is reachable over Tailcat.
  key:   ephemeral (the address stops working when this process exits)
  exec:  disabled (enable with --allow-exec)
  ports: none forwarded (add with --forward-port)
  allow: anyone who has the address
Share the address only over a private channel: it is the credential.
tcomFwWCCcjS5nKNqAod034n...
Press Ctrl-C to stop.
```

The `tc...` address is the only line on stdout, so a script can capture it. Everything else goes to stderr.

On the other machine:

```bash
telemaco remote status tcXXXX
```

```
Transport: tailcat
Path:      direct, 9.2ms
Remote:    mac-home
OS:        macos aarch64
Telemaco:  0.2.1
Protocol:  v1
Ports:     none forwarded
Exec:      disabled
```

`Path` shows whether the tunnel is peer-to-peer or relayed through DERP.

```bash
telemaco remote ping tcXXXX -c 5
```

### Run programs

Start the server with `--allow-exec`:

```bash
telemaco remote serve --allow-exec
```

Then, from the client:

```bash
telemaco remote exec tcXXXX -- uname -a
telemaco remote exec tcXXXX -- docker ps
telemaco remote exec tcXXXX --cwd /srv/app --env RUST_LOG=debug --timeout 600 -- cargo test
```

Output streams as it is produced, stdout and stderr kept apart. The program and its arguments travel as a list and are never passed through a shell, so `*`, `$(...)` and `;` reach the program literally. To use a shell, name it:

```bash
telemaco remote exec tcXXXX -- sh -c 'ls | wc -l'
```

Ctrl-C (or closing the local output, as in `| head`) kills the remote program.

`remote exec` exits with the remote program's status, or:

| Status | Meaning |
|---|---|
| 128 + N | killed by signal N |
| 127 | the program could not be started (not found, bad `--cwd`) |
| 124 | killed by `--timeout` |
| 130 | cancelled (Ctrl-C, or local output closed) |
| 255 | the transport failed, or the agent refused (for example exec disabled) |

### Forward a port

Expose a local port on the server, explicitly:

```bash
telemaco remote serve --forward-port 5432
```

Then, from the client:

```bash
telemaco remote forward tcXXXX 5432
psql -h 127.0.0.1 -p 5432 ...
```

`remote forward` first asks the agent whether the port is forwarded, so a misconfiguration fails at once with a clear message. It listens on `127.0.0.1` by default. `--local-port 0` lets the OS pick a free port (printed on stdout), and `--bind` changes the listen address. Any non-loopback bind, such as `0.0.0.0`, prints a warning, because other machines on your network can then use the port.

Only the named ports on the server's `localhost` are exposed. Nothing else on the server or its LAN is reachable, and the server never acts as an exit node.

### Local target

`local` works everywhere a `tc...` address does, except `forward`. It runs an agent on this machine with no network, which is handy for trying commands or scripting them:

```bash
telemaco remote exec local -- uname -a
```

## Security model

- **The address is the credential.** A tailcat address contains the server's WireGuard public key and a pre-shared key, and holding it is what lets a client connect. Share it only over private channels. Telemaco redacts it everywhere it logs (`tcomFw...****`), including in Tailcat's own output, and prints it in full exactly once: on `serve`'s stdout.
- **Ephemeral by default.** `serve` always passes `--key=new` to tailcat, because plain `tailcat serve` would silently reuse a saved `default` key if one exists. The address dies with the process.
- **Saved keys.** `--key NAME` uses a key made with `tailcat genkey --key=NAME`, so the address survives restarts. Anyone you have ever given it to can reconnect, so pair it with `--allow`.
- **Allow lists.** `--allow nodekey:...` (repeatable) accepts only those client keys, at the WireGuard layer, before Telemaco sees a byte. Generate a client key with `tailcat genkey --client --key=client-default`; client commands then use it automatically.
- **Exec is opt-in.** Without `--allow-exec` the agent refuses exec requests. `--allow-exec` together with a saved `--key` and no `--allow` is refused outright: that combination would be a permanent, unauthenticated shell.
- **Least exposure.** Tailcat's `exec` service starts one agent per connection with the connection as its stdin/stdout. Neither side opens a TCP listener, so other users on either machine cannot reach the agent. Only `--forward-port` ports are proxied.
- **Audit.** The `serve` terminal shows one line per session and per exec, with a truncated peer key and the program name. Arguments are never logged, since they may carry secrets.
- **Untrusted input.** Every frame is capped at 1 MiB and decoded into a closed set of message types that reject unknown fields. Exec requests are validated: no NUL bytes, valid variable names, absolute `--cwd`, bounded argument and variable counts. Strings shown from the remote are stripped of terminal control sequences.
- **Process listing.** `tailcat` takes the address as a command-line argument, so other local users can see it with `ps` while a client command runs. On shared machines, prefer `--allow` so the address alone is not enough.

## File transfer

Telemaco does not wrap file transfer. Tailcat already has a write-only drop box and SFTP-based copying:

```bash
tailcat recv ~/inbox                  # receiver prints its own address
tailcat cp report.pdf tcYYYY:         # sender
```

See Tailcat's README for `serve files`, `ls` and `cp`.

## Limitations

- The remote program's stdin is empty; interactive programs are not supported.
- Cancelling or timing out kills the program itself. Background processes it started keep running, as after an `ssh` session.
- The program inherits the environment of the user running `remote serve`, plus any `--env`.
- Each command opens its own tunnel, so the first round trip includes Tailcat's DERP bootstrap (usually under a second).

## Troubleshooting

`Tailcat transport is unavailable`: `tailcat` is not on `PATH`. Install it or set `TELEMACO_TAILCAT_BIN`.

`tailcat vX is not supported`: upgrade Tailcat to v0.7.0 or newer.

`the remote agent could not be reached`: the message that follows is Tailcat's own error. Common causes are a mistyped or expired address (the server was restarted with an ephemeral key) or a client key missing from the server's `--allow` list.

## How it works

```
telemaco remote exec tcXXXX -- uname -a
  -> tailcat tcXXXX 7431                 stdin/stdout become the tunnel
       ~ WireGuard over UDP, DERP fallback ~
  -> tailcat serve --key=new --json exec -- telemaco remote agent --allow-exec
       -> telemaco remote agent          one process per connection, protocol on stdio
            -> uname -a                  no shell
```

The protocol is small and transport-agnostic. Each frame is a 4-byte big-endian length followed by a JSON message. The client sends `hello` with the protocol version (currently 1); the agent answers `hello` or `unsupported_version`. Then come `ping`, `status`, and `exec`, which answers with a stream of `output` chunks (base64) and one `exited`. A `cancel` stops a running exec. The same protocol runs unchanged over the `local` transport, and a future transport only has to provide a byte stream.
