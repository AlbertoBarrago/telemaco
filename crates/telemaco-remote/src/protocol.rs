//! Telemaco remote protocol, version 1.
//!
//! Wire format: each message is a frame of a 4-byte big-endian length
//! followed by that many bytes of UTF-8 JSON. JSON through serde into closed
//! enums with `deny_unknown_fields` means untrusted input can only ever
//! become one of the listed message shapes; there is no type-directed
//! deserialization. Frames are capped at [`MAX_FRAME_BYTES`].
//!
//! Session: the client sends [`Request::Hello`] first; the agent answers
//! [`Response::Hello`] or an `unsupported_version` error and closes. After
//! that, each request gets exactly one response, in order.

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const PROTOCOL_VERSION: u32 = 1;

/// Largest frame either side accepts or sends. Generous for status and
/// command output chunks, small enough that a hostile peer cannot make the
/// other side allocate much.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// Longest free-text field (error messages, host names) a peer may send.
pub const MAX_TEXT_CHARS: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Hello {
        version: u32,
    },
    Ping {
        nonce: u64,
    },
    /// A struct variant rather than a unit one on purpose: serde ignores
    /// `deny_unknown_fields` for unit variants of internally tagged enums,
    /// so `{"type":"status","x":1}` would otherwise be accepted.
    Status {},
    /// Run one program. The agent answers with any number of
    /// [`Response::Output`] frames followed by exactly one
    /// [`Response::Exited`], or with a single [`Response::Error`].
    Exec(ExecRequest),
    /// Only valid while an exec is running: kill it. The agent still ends
    /// the exec with [`Response::Exited`] (`end: cancelled`).
    Cancel {},
}

/// A structured command: never a shell string. If the caller wants a shell
/// they name it as the program (`sh`, `-c`, `...`), explicitly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecRequest {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Added to (or overriding) the agent's inherited environment.
    #[serde(default)]
    pub env: Vec<EnvVar>,
    /// Absolute working directory; the agent's own when absent.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Kill the program after this many seconds.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvVar {
    pub name: String,
    pub value: String,
}

pub const MAX_EXEC_ARGS: usize = 1024;
pub const MAX_EXEC_ENV: usize = 256;
pub const MAX_PROGRAM_CHARS: usize = 4096;

impl ExecRequest {
    /// Rejects anything the OS would misinterpret or that has no business
    /// crossing the wire: NUL bytes (which truncate C strings), malformed
    /// variable names, relative working directories, absurd counts.
    pub fn validate(&self) -> Result<(), String> {
        let no_nul = |what: &str, s: &str| {
            if s.contains('\0') {
                Err(format!("{what} contains a NUL byte"))
            } else {
                Ok(())
            }
        };
        if self.program.is_empty() {
            return Err("program is empty".into());
        }
        if self.program.chars().count() > MAX_PROGRAM_CHARS {
            return Err("program name is too long".into());
        }
        no_nul("program", &self.program)?;
        if self.args.len() > MAX_EXEC_ARGS {
            return Err(format!("more than {MAX_EXEC_ARGS} arguments"));
        }
        for a in &self.args {
            no_nul("argument", a)?;
        }
        if self.env.len() > MAX_EXEC_ENV {
            return Err(format!("more than {MAX_EXEC_ENV} environment variables"));
        }
        for v in &self.env {
            let mut chars = v.name.chars();
            let valid_name = chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
            if !valid_name {
                return Err("environment variable names must match [A-Za-z_][A-Za-z0-9_]*".into());
            }
            no_nul("environment value", &v.value)?;
        }
        if let Some(cwd) = &self.cwd {
            no_nul("cwd", cwd)?;
            if !std::path::Path::new(cwd).is_absolute() {
                return Err("cwd must be an absolute path".into());
            }
        }
        if self.timeout_secs == Some(0) {
            return Err("timeout must be at least 1 second".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Response {
    Hello {
        version: u32,
        telemaco_version: String,
    },
    Pong {
        nonce: u64,
    },
    Status(StatusInfo),
    /// A chunk of a running program's output, base64 encoded (output is
    /// arbitrary bytes, JSON strings are not).
    Output {
        stream: OutputStream,
        data: String,
    },
    Exited(ExecExit),
    Error(RemoteError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// How an exec ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecExit {
    /// The exit code, when the program exited normally.
    pub code: Option<i32>,
    /// The terminating signal on unix, when there was one.
    pub signal: Option<i32>,
    pub end: ExecEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecEnd {
    /// The program finished on its own.
    Exited,
    /// The agent killed it at `timeout_secs`.
    TimedOut,
    /// The agent killed it on the client's request.
    Cancelled,
}

impl Response {
    pub fn output(stream: OutputStream, bytes: &[u8]) -> Self {
        use base64::Engine as _;
        Response::Output {
            stream,
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        }
    }
}

/// Decodes the payload of a [`Response::Output`].
pub fn decode_output(data: &str) -> Result<Vec<u8>, ProtocolError> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| ProtocolError::Malformed(format!("output chunk: {e}")))
}

/// What an authenticated client may learn about the agent's machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusInfo {
    pub hostname: Option<String>,
    pub os: String,
    pub arch: String,
    pub telemaco_version: String,
    pub protocol_version: u32,
    pub exec_enabled: bool,
    /// Local ports of the agent's machine the operator chose to expose
    /// (`remote serve --forward-port`), reachable with `remote forward`.
    pub forwarded_ports: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(deny_unknown_fields)]
#[error("remote error ({code}): {message}")]
pub struct RemoteError {
    pub code: ErrorCode,
    pub message: String,
}

impl RemoteError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        let mut message = message.into();
        truncate_chars(&mut message, MAX_TEXT_CHARS);
        Self { code, message }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The frame did not decode to a known request.
    BadRequest,
    /// The client speaks a protocol version the agent does not.
    UnsupportedVersion,
    /// The agent understood the request but its policy refuses it.
    Forbidden,
    /// The agent failed while handling a valid request.
    Internal,
    /// The program could not be started (not found, not executable, bad cwd).
    ExecFailed,
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ErrorCode::BadRequest => "bad_request",
            ErrorCode::UnsupportedVersion => "unsupported_version",
            ErrorCode::Forbidden => "forbidden",
            ErrorCode::Internal => "internal",
            ErrorCode::ExecFailed => "exec_failed",
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("connection closed by the remote side")]
    Closed,
    #[error("frame of {0} bytes exceeds the {MAX_FRAME_BYTES}-byte limit")]
    FrameTooLarge(usize),
    #[error("empty frame")]
    EmptyFrame,
    #[error("malformed message: {0}")]
    Malformed(String),
    #[error("remote speaks protocol v{theirs}, this side speaks v{ours}")]
    VersionMismatch { ours: u32, theirs: u32 },
    #[error("unexpected response from the remote agent: {0}")]
    Unexpected(String),
    #[error(transparent)]
    Remote(#[from] RemoteError),
    #[error("protocol I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl ProtocolError {
    /// Whether the stream ended underneath the protocol, in which case the
    /// transport (e.g. tailcat's stderr) knows why and should be asked.
    pub fn is_disconnect(&self) -> bool {
        match self {
            ProtocolError::Closed => true,
            ProtocolError::Io(e) => matches!(
                e.kind(),
                std::io::ErrorKind::UnexpectedEof
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::ConnectionReset
            ),
            _ => false,
        }
    }
}

/// Reads one frame. `Ok(None)` is a clean EOF on a frame boundary; EOF in
/// the middle of a frame is an error.
pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> Result<Option<Vec<u8>>, ProtocolError> {
    let mut len = [0u8; 4];
    let mut got = 0;
    while got < len.len() {
        let n = r.read(&mut len[got..]).await?;
        if n == 0 {
            return if got == 0 {
                Ok(None)
            } else {
                Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof).into())
            };
        }
        got += n;
    }
    let len = u32::from_be_bytes(len) as usize;
    if len == 0 {
        return Err(ProtocolError::EmptyFrame);
    }
    if len > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge(len));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await?;
    Ok(Some(body))
}

pub async fn write_frame<W: AsyncWrite + Unpin>(
    w: &mut W,
    body: &[u8],
) -> Result<(), ProtocolError> {
    if body.len() > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge(body.len()));
    }
    w.write_all(&(body.len() as u32).to_be_bytes()).await?;
    w.write_all(body).await?;
    w.flush().await?;
    Ok(())
}

pub fn decode<'a, T: Deserialize<'a>>(body: &'a [u8]) -> Result<T, ProtocolError> {
    serde_json::from_slice(body).map_err(|e| {
        let mut msg = e.to_string();
        truncate_chars(&mut msg, 256);
        ProtocolError::Malformed(msg)
    })
}

pub async fn send<W: AsyncWrite + Unpin, T: Serialize>(
    w: &mut W,
    msg: &T,
) -> Result<(), ProtocolError> {
    let body = serde_json::to_vec(msg).map_err(|e| ProtocolError::Malformed(e.to_string()))?;
    write_frame(w, &body).await
}

/// Reads and decodes one message; a clean EOF becomes [`ProtocolError::Closed`].
pub async fn recv<R: AsyncRead + Unpin, T: for<'a> Deserialize<'a>>(
    r: &mut R,
) -> Result<T, ProtocolError> {
    let body = read_frame(r).await?.ok_or(ProtocolError::Closed)?;
    decode(&body)
}

fn truncate_chars(s: &mut String, max: usize) {
    if let Some((idx, _)) = s.char_indices().nth(max) {
        s.truncate(idx);
    }
}

/// Makes a peer-supplied string safe to print on a terminal: control
/// characters (including ANSI escape introducers) become `?`, and length is
/// capped. A malicious agent must not be able to rewrite the user's screen.
pub fn sanitize_for_display(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect();
    truncate_chars(&mut out, 256);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status() -> StatusInfo {
        StatusInfo {
            hostname: Some("mac-home".into()),
            os: "macos".into(),
            arch: "aarch64".into(),
            telemaco_version: "0.2.1".into(),
            protocol_version: PROTOCOL_VERSION,
            exec_enabled: false,
            forwarded_ports: vec![5432],
        }
    }

    #[test]
    fn wire_shape_is_stable() {
        let json = serde_json::to_string(&Request::Hello { version: 1 }).unwrap();
        assert_eq!(json, r#"{"type":"hello","version":1}"#);
        let json = serde_json::to_string(&Request::Status {}).unwrap();
        assert_eq!(json, r#"{"type":"status"}"#);
        let json = serde_json::to_string(&Response::Error(RemoteError::new(
            ErrorCode::Forbidden,
            "no",
        )))
        .unwrap();
        assert_eq!(
            json,
            r#"{"type":"error","code":"forbidden","message":"no"}"#
        );
    }

    #[test]
    fn every_message_round_trips() {
        let requests = [
            Request::Hello { version: 1 },
            Request::Ping { nonce: 7 },
            Request::Status {},
        ];
        for r in requests {
            let back: Request = decode(&serde_json::to_vec(&r).unwrap()).unwrap();
            assert_eq!(back, r);
        }
        let responses = [
            Response::Hello {
                version: 1,
                telemaco_version: "0.2.1".into(),
            },
            Response::Pong { nonce: 7 },
            Response::Status(status()),
            Response::Error(RemoteError::new(ErrorCode::BadRequest, "x")),
        ];
        for r in responses {
            let back: Response = decode(&serde_json::to_vec(&r).unwrap()).unwrap();
            assert_eq!(back, r);
        }
    }

    #[test]
    fn malformed_messages_are_rejected() {
        let bad: &[&[u8]] = &[
            b"not json",
            b"{}",
            br#"{"type":"shell","cmd":"rm -rf /"}"#,
            br#"{"type":"ping"}"#,
            br#"{"type":"ping","nonce":-1}"#,
            br#"{"type":"status","extra":true}"#,
            br#"{"type":"hello","version":1,"admin":true}"#,
            b"\xff\xfe",
        ];
        for body in bad {
            let err = decode::<Request>(body).unwrap_err();
            assert!(
                matches!(err, ProtocolError::Malformed(_)),
                "{body:?} -> {err}"
            );
        }
        let extra = br#"{"type":"status","hostname":null,"os":"x","arch":"y","telemaco_version":"z","protocol_version":1,"exec_enabled":true,"forwarded_ports":[],"root":1}"#;
        assert!(decode::<Response>(extra).is_err());
    }

    #[tokio::test]
    async fn frames_round_trip_and_eof_is_clean() {
        let (mut a, mut b) = tokio::io::duplex(64);
        tokio::spawn(async move {
            send(&mut a, &Request::Ping { nonce: 1 }).await.unwrap();
            send(&mut a, &Request::Status {}).await.unwrap();
        });
        assert_eq!(
            recv::<_, Request>(&mut b).await.unwrap(),
            Request::Ping { nonce: 1 }
        );
        assert_eq!(
            recv::<_, Request>(&mut b).await.unwrap(),
            Request::Status {}
        );
        assert!(matches!(
            recv::<_, Request>(&mut b).await,
            Err(ProtocolError::Closed)
        ));
    }

    #[tokio::test]
    async fn oversized_and_empty_frames_are_refused_before_allocating() {
        let mut input: &[u8] = &(u32::MAX).to_be_bytes();
        assert!(matches!(
            read_frame(&mut input).await,
            Err(ProtocolError::FrameTooLarge(_))
        ));
        let mut input: &[u8] = &0u32.to_be_bytes();
        assert!(matches!(
            read_frame(&mut input).await,
            Err(ProtocolError::EmptyFrame)
        ));
        let mut sink = Vec::new();
        let big = vec![b' '; MAX_FRAME_BYTES + 1];
        assert!(matches!(
            write_frame(&mut sink, &big).await,
            Err(ProtocolError::FrameTooLarge(_))
        ));
        assert!(sink.is_empty());
    }

    #[tokio::test]
    async fn truncated_frames_are_errors_not_eof() {
        let mut half_len: &[u8] = &[0, 0];
        let err = read_frame(&mut half_len).await.unwrap_err();
        assert!(err.is_disconnect());
        let mut frame = 10u32.to_be_bytes().to_vec();
        frame.extend_from_slice(b"abc");
        let err = read_frame(&mut frame.as_slice()).await.unwrap_err();
        assert!(err.is_disconnect(), "{err}");
    }

    fn exec(program: &str) -> ExecRequest {
        ExecRequest {
            program: program.into(),
            args: vec![],
            env: vec![],
            cwd: None,
            timeout_secs: None,
        }
    }

    #[test]
    fn exec_messages_round_trip_with_arguments_intact() {
        let req = Request::Exec(ExecRequest {
            program: "printf".into(),
            args: vec![
                "%s|".into(),
                "two words".into(),
                "$(id)".into(),
                "".into(),
                "é\n".into(),
            ],
            env: vec![EnvVar {
                name: "FOO".into(),
                value: "a=b c".into(),
            }],
            cwd: Some("/tmp".into()),
            timeout_secs: Some(5),
        });
        let back: Request = decode(&serde_json::to_vec(&req).unwrap()).unwrap();
        assert_eq!(back, req);
        let minimal: Request = decode(br#"{"type":"exec","program":"uname"}"#).unwrap();
        assert_eq!(minimal, Request::Exec(exec("uname")));
        let out = Response::output(OutputStream::Stderr, b"\x00\xffbin");
        let Response::Output { stream, data } = decode(&serde_json::to_vec(&out).unwrap()).unwrap()
        else {
            panic!()
        };
        assert_eq!(stream, OutputStream::Stderr);
        assert_eq!(decode_output(&data).unwrap(), b"\x00\xffbin");
        assert!(decode_output("not base64!").is_err());
        let exited = Response::Exited(ExecExit {
            code: None,
            signal: Some(9),
            end: ExecEnd::Cancelled,
        });
        let back: Response = decode(&serde_json::to_vec(&exited).unwrap()).unwrap();
        assert_eq!(back, exited);
    }

    #[test]
    fn exec_rejects_a_shell_string_field_and_unknown_fields() {
        assert!(decode::<Request>(br#"{"type":"exec","program":"ls","shell":true}"#).is_err());
        assert!(decode::<Request>(br#"{"type":"exec","command":"ls -la"}"#).is_err());
        assert!(decode::<Request>(br#"{"type":"exec","program":"ls","args":"-la"}"#).is_err());
        assert!(decode::<Request>(br#"{"type":"cancel","now":true}"#).is_err());
    }

    #[test]
    fn exec_validation() {
        assert!(exec("uname").validate().is_ok());
        assert!(exec("").validate().is_err());
        assert!(exec("ls\0rm").validate().is_err());
        let mut r = exec("ls");
        r.args = vec!["a\0b".into()];
        assert!(r.validate().is_err());
        let mut r = exec("ls");
        r.args = vec![String::new(); MAX_EXEC_ARGS + 1];
        assert!(r.validate().is_err());
        for bad in ["", "1X", "A-B", "A B", "A=B"] {
            let mut r = exec("ls");
            r.env = vec![EnvVar {
                name: bad.into(),
                value: "v".into(),
            }];
            assert!(r.validate().is_err(), "{bad:?}");
        }
        let mut r = exec("ls");
        r.env = vec![EnvVar {
            name: "_OK_1".into(),
            value: "v".into(),
        }];
        assert!(r.validate().is_ok());
        let mut r = exec("ls");
        r.cwd = Some("relative/dir".into());
        assert!(r.validate().is_err());
        let mut r = exec("ls");
        r.timeout_secs = Some(0);
        assert!(r.validate().is_err());
    }

    #[test]
    fn error_messages_are_bounded() {
        let e = RemoteError::new(ErrorCode::Internal, "é".repeat(MAX_TEXT_CHARS * 2));
        assert_eq!(e.message.chars().count(), MAX_TEXT_CHARS);
    }

    #[test]
    fn display_sanitizing_strips_terminal_escapes() {
        assert_eq!(sanitize_for_display("host\x1b[2J\r\n"), "host?[2J??");
        assert_eq!(sanitize_for_display(&"a".repeat(1000)).len(), 256);
    }
}
