//! End-to-end tests for `telemaco remote`, with no network.
//!
//! A `/bin/sh` script stands in for `tailcat` (via TELEMACO_TAILCAT_BIN). In
//! client mode it simply execs `telemaco remote agent`, which is what the
//! real tunnel amounts to from the protocol's point of view, so the whole
//! stack (CLI, transport selection, subprocess, protocol, agent) runs for real.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const ADDR: &str = "tcomFwWCCcjS5nKNqAod034nWoJZW0LZqDhhC8U_dKdnDRYQ8uNGFpGQEu";
const REDACTED: &str = "tcomFw...****";

fn telemaco() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_telemaco"));
    cmd.env("TELEMACO_NO_UPDATE_CHECK", "1");
    cmd
}

fn scratch_dir(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("telemaco-remote-cli-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fake_tailcat(dir: &Path, client_body: &str) -> PathBuf {
    let path = dir.join("tailcat");
    let script = format!(
        "#!/bin/sh\n\
         case \"$1\" in\n\
           version) echo v0.7.0 ;;\n\
           ping) echo 'pong in 1.2ms via 203.0.113.7:41641' ;;\n\
           *) {client_body} ;;\n\
         esac\n"
    );
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn text(out: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn status_over_the_local_transport() {
    let out = telemaco()
        .args(["remote", "status", "local"])
        .output()
        .unwrap();
    let (stdout, stderr) = text(&out);
    assert!(out.status.success(), "{stderr}");
    assert!(stdout.contains("Transport: local"), "{stdout}");
    assert!(stdout.contains("Protocol:  v1"), "{stdout}");
    // Same user, same machine, no network hop: the local agent allows exec.
    assert!(stdout.contains("Exec:      enabled"), "{stdout}");
    assert!(
        !stdout.contains("Path:"),
        "local has no network path: {stdout}"
    );
}

#[test]
fn status_and_ping_over_a_fake_tailcat() {
    let dir = scratch_dir("ok");
    let agent = env!("CARGO_BIN_EXE_telemaco");
    let bin = fake_tailcat(&dir, &format!("exec '{agent}' remote agent"));

    let out = telemaco()
        .env("TELEMACO_TAILCAT_BIN", &bin)
        .args(["remote", "status", ADDR])
        .output()
        .unwrap();
    let (stdout, stderr) = text(&out);
    assert!(out.status.success(), "{stderr}");
    assert!(stdout.contains("Transport: tailcat"), "{stdout}");
    assert!(stdout.contains("Path:      direct, 1.2ms"), "{stdout}");
    assert!(
        !stdout.contains("203.0.113.7"),
        "peer endpoint must not be shown: {stdout}"
    );
    assert!(!format!("{stdout}{stderr}").contains(ADDR));

    let out = telemaco()
        .env("TELEMACO_TAILCAT_BIN", &bin)
        .args(["remote", "ping", ADDR, "-c", "2"])
        .output()
        .unwrap();
    let (stdout, stderr) = text(&out);
    assert!(out.status.success(), "{stderr}");
    assert!(stdout.contains("2 pings over tailcat"), "{stdout}");
}

#[test]
fn missing_tailcat_explains_itself() {
    let out = telemaco()
        .env_remove("TELEMACO_TAILCAT_BIN")
        .env("PATH", "/nonexistent")
        .args(["remote", "status", ADDR])
        .output()
        .unwrap();
    let (_, stderr) = text(&out);
    assert!(!out.status.success());
    assert!(
        stderr.contains("Tailcat transport is unavailable"),
        "{stderr}"
    );
    assert!(stderr.contains("Install tailcat"), "{stderr}");
    assert!(!stderr.contains(ADDR));
}

#[test]
fn tailcat_failure_is_reported_verbatim_and_redacted() {
    let dir = scratch_dir("fail");
    let bin = fake_tailcat(
        &dir,
        "echo \"tailcat: handshake with $1 timed out\" >&2; exit 1",
    );
    let out = telemaco()
        .env("TELEMACO_TAILCAT_BIN", &bin)
        .args(["remote", "status", ADDR])
        .output()
        .unwrap();
    let (_, stderr) = text(&out);
    assert!(!out.status.success());
    assert!(stderr.contains("could not be reached"), "{stderr}");
    assert!(
        stderr.contains(&format!("handshake with {REDACTED} timed out")),
        "{stderr}"
    );
    assert!(!stderr.contains(ADDR), "{stderr}");
}

#[test]
fn malformed_target_is_rejected_before_spawning_anything() {
    let out = telemaco()
        .env("TELEMACO_TAILCAT_BIN", "/nonexistent/tailcat")
        .args(["remote", "status", "tcbad;rm -rf ~"])
        .output()
        .unwrap();
    let (_, stderr) = text(&out);
    assert!(!out.status.success());
    assert!(stderr.contains("expected a tailcat address"), "{stderr}");
}

/// Runs `remote serve <extra>` against a fake tailcat, stops it with SIGINT,
/// checks tailcat was torn down, and returns tailcat's argv and our stderr.
fn run_serve(tag: &str, extra: &[&str]) -> (Vec<String>, String) {
    let dir = scratch_dir(tag);
    let pidfile = dir.join("tailcat.pid");
    let argvfile = dir.join("tailcat.argv");
    let path = dir.join("tailcat");
    let script = format!(
        "#!/bin/sh\n\
         case \"$1\" in\n\
           version) echo v0.7.0; exit 0 ;;\n\
         esac\n\
         echo $$ > '{pid}'\n\
         printf '%s\\n' \"$@\" > '{argv}'\n\
         echo '# 🐈 Server listening with new address: {ADDR}' >&2\n\
         echo '{{\"listenAddr\":\"{ADDR}\"}}'\n\
         exec sleep 60\n",
        pid = pidfile.display(),
        argv = argvfile.display(),
    );
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut child = telemaco()
        .env("TELEMACO_TAILCAT_BIN", &path)
        .args(["remote", "serve"])
        .args(extra)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    // The address is the one line on stdout.
    let mut stdout = child.stdout.take().unwrap();
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    while std::io::Read::read(&mut stdout, &mut byte).unwrap() == 1 && byte[0] != b'\n' {
        line.push(byte[0]);
    }
    assert_eq!(String::from_utf8(line).unwrap(), ADDR);

    let argv: Vec<String> = std::fs::read_to_string(&argvfile)
        .unwrap()
        .lines()
        .map(String::from)
        .collect();

    let tailcat_pid: u32 = std::fs::read_to_string(&pidfile)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let status = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    let out = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "{stderr}");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let alive = Command::new("kill")
            .args(["-0", &tailcat_pid.to_string()])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success();
        if !alive {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "tailcat {tailcat_pid} outlived remote serve"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    (argv, stderr)
}

#[test]
fn serve_prints_the_address_and_cleans_up_tailcat_on_sigint() {
    let (argv, stderr) = run_serve("serve", &[]);
    assert_eq!(&argv[..5], ["serve", "--key=new", "--json", "exec", "--"]);
    assert!(argv[5].ends_with("telemaco"), "{argv:?}");
    assert_eq!(&argv[6..], ["remote", "agent"]);
    assert!(stderr.contains("ephemeral"), "{stderr}");
    assert!(stderr.contains("exec:  disabled"), "{stderr}");
    // tailcat's own announcement is relayed, but redacted.
    assert!(stderr.contains(REDACTED), "{stderr}");
    assert!(!stderr.contains(ADDR), "{stderr}");
}

#[test]
fn serve_allow_exec_reaches_the_agent_and_is_announced() {
    let (argv, stderr) = run_serve("serve-exec", &["--allow-exec"]);
    assert_eq!(&argv[6..], ["remote", "agent", "--allow-exec"]);
    assert!(stderr.contains("exec:  ENABLED"), "{stderr}");
}

fn exec_bin(dir_tag: &str, agent_flags: &str) -> PathBuf {
    let dir = scratch_dir(dir_tag);
    let agent = env!("CARGO_BIN_EXE_telemaco");
    fake_tailcat(&dir, &format!("exec '{agent}' remote agent {agent_flags}"))
}

#[test]
fn exec_over_tailcat_streams_output_and_propagates_the_exit_code() {
    let bin = exec_bin("exec", "--allow-exec");
    let out = telemaco()
        .env("TELEMACO_TAILCAT_BIN", &bin)
        .args(["remote", "exec", ADDR, "--env", "WHO=a b", "--"])
        .args([
            "sh",
            "-c",
            "printf '%s|' \"$WHO\" \"$0\"; echo oops >&2; exit 42",
            "x;y",
        ])
        .output()
        .unwrap();
    let (stdout, stderr) = text(&out);
    assert_eq!(out.status.code(), Some(42), "{stderr}");
    assert_eq!(stdout, "a b|x;y|");
    assert!(stderr.contains("oops"), "{stderr}");
    assert!(!stderr.contains(ADDR), "{stderr}");
}

#[test]
fn exec_refused_by_an_agent_without_allow_exec() {
    let bin = exec_bin("noexec", "");
    let out = telemaco()
        .env("TELEMACO_TAILCAT_BIN", &bin)
        .args(["remote", "exec", ADDR, "--", "id"])
        .output()
        .unwrap();
    let (stdout, stderr) = text(&out);
    assert_eq!(out.status.code(), Some(255), "{stderr}");
    assert!(stdout.is_empty());
    assert!(stderr.contains("forbidden"), "{stderr}");
    assert!(stderr.contains("--allow-exec"), "{stderr}");
}

#[test]
fn exec_exit_statuses_for_non_program_outcomes() {
    let run = |args: &[&str]| {
        telemaco()
            .args(["remote", "exec", "local"])
            .args(args)
            .output()
            .unwrap()
    };
    assert_eq!(
        run(&["--", "/nonexistent/program"]).status.code(),
        Some(127)
    );
    assert_eq!(
        run(&["--timeout", "1", "--", "sleep", "10"]).status.code(),
        Some(124)
    );
    assert_eq!(
        run(&["--", "sh", "-c", "kill -9 $$"]).status.code(),
        Some(137)
    );
    assert_eq!(
        run(&["--env", "BAD NAME=1", "--", "true"]).status.code(),
        Some(1)
    );
}

#[test]
fn ctrl_c_cancels_the_remote_program() {
    let dir = scratch_dir("cancel");
    let pidfile = dir.join("program.pid");
    let mut child = telemaco()
        .args(["remote", "exec", "local", "--", "sh", "-c"])
        .arg(format!(
            "echo $$ > '{}'; echo started; exec sleep 60",
            pidfile.display()
        ))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut buf = [0u8; 8];
    std::io::Read::read_exact(&mut stdout, &mut buf).unwrap();
    assert_eq!(&buf, b"started\n");
    let program_pid = std::fs::read_to_string(&pidfile)
        .unwrap()
        .trim()
        .to_string();

    Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    let started = Instant::now();
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(130));
    assert!(started.elapsed() < Duration::from_secs(10));
    let alive = Command::new("kill")
        .args(["-0", &program_pid])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success();
    assert!(!alive, "remote program {program_pid} survived Ctrl-C");
}

#[test]
fn serve_refuses_exec_on_a_saved_key_without_an_allow_list() {
    let dir = scratch_dir("savedkey");
    let bin = fake_tailcat(&dir, "exit 0");
    let out = telemaco()
        .env("TELEMACO_TAILCAT_BIN", &bin)
        .args(["remote", "serve", "--allow-exec", "--key", "home"])
        .output()
        .unwrap();
    let (_, stderr) = text(&out);
    assert!(!out.status.success());
    assert!(stderr.contains("needs --allow"), "{stderr}");
}
