//! Child-process plumbing shared by the process-backed transports.
//!
//! Tailcat and the local agent both speak over a child's stdin/stdout, so a
//! [`Connection`] is a child process with piped stdio. Invariants:
//! - argv is always a vector, never a shell string;
//! - the child is killed if the connection is dropped (`kill_on_drop`);
//! - stderr is drained continuously into a bounded tail buffer, so a chatty
//!   child can neither block on a full pipe nor grow memory without bound.

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadBuf};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::task::JoinHandle;

use super::TransportError;
use crate::address::TailcatAddress;

/// How much of a child's stderr is kept for error reports (the tail).
const STDERR_TAIL_BYTES: usize = 16 * 1024;
/// Cap on captured stdout for short-lived helper runs (`tailcat version`).
const CAPTURE_LIMIT_BYTES: u64 = 64 * 1024;

/// One argv element. Secrets are typed so `Debug` output and error text
/// redact them without every call site having to remember.
#[derive(Debug, Clone)]
pub enum Arg {
    Plain(OsString),
    Secret(TailcatAddress),
}

/// A program plus its arguments, ready to spawn without a shell.
#[derive(Debug, Clone)]
pub struct ProcessSpec {
    pub program: PathBuf,
    pub args: Vec<Arg>,
}

impl ProcessSpec {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
        }
    }

    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(Arg::Plain(arg.into()));
        self
    }

    pub fn secret(mut self, addr: TailcatAddress) -> Self {
        self.args.push(Arg::Secret(addr));
        self
    }

    /// Program name for messages: the file name, not the full path.
    pub(crate) fn display_name(&self) -> String {
        self.program
            .file_name()
            .unwrap_or(self.program.as_os_str())
            .to_string_lossy()
            .into_owned()
    }

    /// Scrubs every secret argument out of `text`.
    pub(crate) fn redact(&self, text: &str) -> String {
        self.args
            .iter()
            .fold(text.to_string(), |acc, arg| match arg {
                Arg::Secret(addr) => addr.redact_in(&acc),
                Arg::Plain(_) => acc,
            })
    }

    pub(crate) fn command(&self) -> Command {
        let mut cmd = Command::new(&self.program);
        for arg in &self.args {
            match arg {
                Arg::Plain(s) => cmd.arg(s),
                Arg::Secret(addr) => cmd.arg(addr.expose()),
            };
        }
        cmd.kill_on_drop(true);
        cmd
    }

    pub(crate) fn spawn_error(&self, source: io::Error) -> TransportError {
        TransportError::Spawn {
            program: self.display_name(),
            source,
        }
    }
}

/// A live byte stream over a child's stdin/stdout.
#[derive(Debug)]
pub struct Connection {
    spec: ProcessSpec,
    child: Child,
    reader: ChildStdout,
    /// `None` once [`Connection::close`] has sent EOF.
    writer: Option<ChildStdin>,
    stderr: Option<JoinHandle<Vec<u8>>>,
}

impl Connection {
    /// Spawns `spec` with piped stdio. Must run inside a tokio runtime.
    pub fn spawn(spec: ProcessSpec) -> Result<Self, TransportError> {
        let mut child = spec
            .command()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| spec.spawn_error(e))?;
        let (Some(writer), Some(reader), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(spec.spawn_error(io::Error::other("child stdio was not piped")));
        };
        let stderr = tokio::spawn(drain_tail(stderr, STDERR_TAIL_BYTES));
        tracing::debug!(program = %spec.display_name(), "transport process started");
        Ok(Self {
            spec,
            child,
            reader,
            writer: Some(writer),
            stderr: Some(stderr),
        })
    }

    /// OS process id, while the child is running.
    pub fn id(&self) -> Option<u32> {
        self.child.id()
    }

    /// Graceful end: send EOF, give the child `grace` to exit on its own,
    /// then kill it. The child is always reaped before this returns. A child
    /// that exits non-zero on its own is reported with its stderr; one we had
    /// to kill after the grace period is not an error.
    pub async fn close(mut self, grace: Duration) -> Result<(), TransportError> {
        drop(self.writer.take());
        match tokio::time::timeout(grace, self.child.wait()).await {
            Ok(status) => {
                let status = status?;
                if status.success() {
                    Ok(())
                } else {
                    Err(self.exited(status).await)
                }
            }
            Err(_) => {
                self.child.kill().await?;
                Ok(())
            }
        }
    }

    /// Hard stop, for cancellation and protocol errors: kill and reap now.
    pub async fn abort(mut self) -> Result<(), TransportError> {
        drop(self.writer.take());
        // kill() also waits, so the child never lingers as a zombie.
        match self.child.kill().await {
            Ok(()) => Ok(()),
            // Already exited: nothing left to kill, just reap.
            Err(e) if e.kind() == io::ErrorKind::InvalidInput => {
                self.child.wait().await?;
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Explains why the stream ended early (EOF before the protocol expected
    /// it): waits briefly for the child and returns its exit status and
    /// stderr, which usually carries tailcat's own reason.
    pub async fn failure(mut self) -> TransportError {
        drop(self.writer.take());
        match tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await {
            Ok(Ok(status)) => self.exited(status).await,
            Ok(Err(e)) => e.into(),
            Err(_) => {
                let _ = self.child.kill().await;
                TransportError::Timeout {
                    program: self.spec.display_name(),
                    seconds: 5,
                }
            }
        }
    }

    async fn exited(&mut self, status: std::process::ExitStatus) -> TransportError {
        let stderr = match self.stderr.take() {
            Some(handle) => handle.await.unwrap_or_default(),
            None => Vec::new(),
        };
        TransportError::Exited {
            program: self.spec.display_name(),
            status: describe_status(status),
            stderr: self.spec.redact(&String::from_utf8_lossy(&stderr)),
        }
    }
}

impl AsyncRead for Connection {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.reader).poll_read(cx, buf)
    }
}

impl AsyncWrite for Connection {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.writer.as_mut() {
            Some(w) => Pin::new(w).poll_write(cx, buf),
            None => Poll::Ready(Err(io::ErrorKind::BrokenPipe.into())),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.writer.as_mut() {
            Some(w) => Pin::new(w).poll_flush(cx),
            None => Poll::Ready(Ok(())),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.writer.as_mut() {
            Some(w) => Pin::new(w).poll_shutdown(cx),
            None => Poll::Ready(Ok(())),
        }
    }
}

/// Result of a short-lived helper run such as `tailcat version`.
#[derive(Debug)]
pub struct CapturedOutput {
    pub stdout: String,
    pub stderr: String,
}

/// Runs `spec` to completion with no stdin, bounded output and a deadline.
/// Non-zero exit becomes [`TransportError::Exited`] with the child's stderr.
pub(crate) async fn run_captured(
    spec: &ProcessSpec,
    deadline: Duration,
) -> Result<CapturedOutput, TransportError> {
    let mut child = spec
        .command()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| spec.spawn_error(e))?;
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        return Err(spec.spawn_error(io::Error::other("child stdio was not piped")));
    };
    let run = async {
        let (out, err, status) = tokio::join!(
            read_capped(stdout, CAPTURE_LIMIT_BYTES),
            drain_tail(stderr, STDERR_TAIL_BYTES),
            child.wait(),
        );
        Ok::<_, io::Error>((out?, err, status?))
    };
    let (out, err, status) = match tokio::time::timeout(deadline, run).await {
        Ok(result) => result?,
        Err(_) => {
            // The future holding the child's borrows is gone; kill_on_drop
            // would fire later, but kill and reap explicitly now.
            let _ = child.kill().await;
            return Err(TransportError::Timeout {
                program: spec.display_name(),
                seconds: deadline.as_secs(),
            });
        }
    };
    let stderr = spec.redact(&String::from_utf8_lossy(&err));
    if !status.success() {
        return Err(TransportError::Exited {
            program: spec.display_name(),
            status: describe_status(status),
            stderr,
        });
    }
    Ok(CapturedOutput {
        stdout: spec.redact(&String::from_utf8_lossy(&out)),
        stderr,
    })
}

/// Reads up to `limit` bytes, then keeps draining (and discarding) so the
/// child never blocks on a full pipe.
async fn read_capped(mut r: impl AsyncRead + Unpin, limit: u64) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    (&mut r).take(limit).read_to_end(&mut out).await?;
    tokio::io::copy(&mut r, &mut tokio::io::sink()).await?;
    Ok(out)
}

/// Drains a stream to EOF, keeping only its last `cap` bytes. Read errors end
/// the drain early; what was collected is still returned.
async fn drain_tail(mut r: impl AsyncRead + Unpin, cap: usize) -> Vec<u8> {
    let mut tail = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match r.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                tail.extend_from_slice(&buf[..n]);
                if tail.len() > cap {
                    tail.drain(..tail.len() - cap);
                }
            }
        }
    }
    tail
}

fn describe_status(status: std::process::ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("status {code}"),
        None => format!("{status}"),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::address::tests::SAMPLE;
    use crate::transport::test_support::fake_bin;
    use tokio::io::AsyncWriteExt;

    fn alive(pid: u32) -> bool {
        // A reaped process is gone; `ps` finds nothing. Zombies count as dead.
        let out = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        let stat = String::from_utf8_lossy(&out.stdout);
        let stat = stat.trim();
        !stat.is_empty() && !stat.starts_with('Z')
    }

    #[tokio::test]
    async fn stream_round_trips_through_the_child() {
        let mut conn = Connection::spawn(ProcessSpec::new("/bin/cat")).unwrap();
        conn.write_all(b"ping\n").await.unwrap();
        conn.flush().await.unwrap();
        let mut buf = [0u8; 5];
        conn.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping\n");
        conn.close(Duration::from_secs(5)).await.unwrap();
    }

    #[tokio::test]
    async fn arguments_are_passed_verbatim_without_a_shell() {
        let bin = fake_bin("argv", r#"for a in "$@"; do printf '[%s]\n' "$a"; done"#);
        let spec = ProcessSpec::new(bin)
            .arg("two words")
            .arg("$(touch /tmp/pwned)")
            .arg("; exit 7")
            .arg("");
        let out = run_captured(&spec, Duration::from_secs(5)).await.unwrap();
        assert_eq!(
            out.stdout,
            "[two words]\n[$(touch /tmp/pwned)]\n[; exit 7]\n[]\n"
        );
    }

    #[tokio::test]
    async fn failure_preserves_stderr_and_redacts_the_address() {
        let bin = fake_bin(
            "tailcat",
            r#"echo "tailcat: dial $1: no DERP route" >&2; exit 3"#,
        );
        let addr = TailcatAddress::parse(SAMPLE).unwrap();
        let conn = Connection::spawn(ProcessSpec::new(bin).secret(addr)).unwrap();
        let err = conn.failure().await;
        let msg = err.to_string();
        assert!(matches!(err, TransportError::Exited { .. }), "{msg}");
        assert!(msg.contains("status 3"), "{msg}");
        assert!(msg.contains("no DERP route"), "{msg}");
        assert!(msg.contains("tcomFw...****"), "{msg}");
        assert!(!msg.contains(SAMPLE), "{msg}");
    }

    #[tokio::test]
    async fn spawn_failure_names_the_program() {
        let err = Connection::spawn(ProcessSpec::new("/nonexistent/tailcat")).unwrap_err();
        assert!(matches!(err, TransportError::Spawn { .. }));
        assert!(err.to_string().contains("tailcat"));
    }

    #[tokio::test]
    async fn close_kills_a_child_that_ignores_eof() {
        let bin = fake_bin("stubborn", "exec sleep 30");
        let conn = Connection::spawn(ProcessSpec::new(bin)).unwrap();
        let pid = conn.id().unwrap();
        conn.close(Duration::from_millis(200)).await.unwrap();
        assert!(!alive(pid));
    }

    #[tokio::test]
    async fn abort_reaps_immediately() {
        let bin = fake_bin("stubborn", "exec sleep 30");
        let conn = Connection::spawn(ProcessSpec::new(bin)).unwrap();
        let pid = conn.id().unwrap();
        conn.abort().await.unwrap();
        assert!(!alive(pid));
    }

    #[tokio::test]
    async fn dropping_the_connection_kills_the_child() {
        let bin = fake_bin("stubborn", "exec sleep 30");
        let conn = Connection::spawn(ProcessSpec::new(bin)).unwrap();
        let pid = conn.id().unwrap();
        drop(conn);
        let mut dead = false;
        for _ in 0..50 {
            if !alive(pid) {
                dead = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(dead, "child {pid} survived its dropped connection");
    }

    #[tokio::test]
    async fn captured_run_times_out_and_kills() {
        let bin = fake_bin("slow", "exec sleep 30");
        let err = run_captured(&ProcessSpec::new(bin), Duration::from_millis(200))
            .await
            .unwrap_err();
        assert!(matches!(err, TransportError::Timeout { .. }));
    }

    #[tokio::test]
    async fn stderr_tail_is_bounded() {
        let bin = fake_bin(
            "noisy",
            "i=0; while [ $i -lt 2000 ]; do echo 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx' >&2; i=$((i+1)); done; echo END >&2; exit 1",
        );
        let err = run_captured(&ProcessSpec::new(bin), Duration::from_secs(10))
            .await
            .unwrap_err();
        let TransportError::Exited { stderr, .. } = err else {
            panic!("{err}")
        };
        assert!(stderr.len() <= STDERR_TAIL_BYTES);
        assert!(stderr.trim_end().ends_with("END"));
    }
}
