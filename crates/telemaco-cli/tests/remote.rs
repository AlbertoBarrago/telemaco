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
    assert!(stdout.contains("Exec:      disabled"), "{stdout}");
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

#[test]
fn serve_prints_the_address_and_cleans_up_tailcat_on_sigint() {
    let dir = scratch_dir("serve");
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

    let argv = std::fs::read_to_string(&argvfile).unwrap();
    let argv: Vec<&str> = argv.lines().collect();
    assert_eq!(&argv[..5], ["serve", "--key=new", "--json", "exec", "--"]);
    assert!(argv[5].ends_with("telemaco"), "{argv:?}");
    assert_eq!(&argv[6..], ["remote", "agent"]);

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
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(stderr.contains("ephemeral"), "{stderr}");
    // tailcat's own announcement is relayed, but redacted.
    assert!(stderr.contains(REDACTED), "{stderr}");
    assert!(!stderr.contains(ADDR), "{stderr}");

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
}
