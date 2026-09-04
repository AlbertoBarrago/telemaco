//! `telemaco update`: replace the installed binaries with the latest release.
//!
//! Nothing here runs unless the subcommand is invoked. A browser used for
//! privacy-conscious scraping has no business contacting GitHub on its own, so
//! there is no background check and no startup ping.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

const REPO: &str = "AlbertoBarrago/telemaco";
const USER_AGENT: &str = concat!("telemaco/", env!("TELEMACO_BUILD_VERSION"));

/// The release asset suffix matching how *this* binary was built.
///
/// Without this an update silently downgrades: someone running the stealth
/// build would be handed the plain one and lose the wreq transport with no
/// error and no hint, which is the worst way to lose a feature.
fn variant_suffix() -> &'static str {
    variant_suffix_for(cfg!(feature = "render"), cfg!(feature = "stealth"))
}

/// Split from the `cfg!` lookup so every combination can be tested from one
/// build. Testing it through `cfg!` alone only ever exercises the variant the
/// test binary happens to be compiled as, leaving the other three unchecked.
fn variant_suffix_for(render: bool, stealth: bool) -> &'static str {
    match (render, stealth) {
        (true, true) => "-stealth",
        (true, false) => "",
        (false, true) => "-no-render-stealth",
        (false, false) => "-no-render",
    }
}

fn asset_name() -> Result<String> {
    let arch = match std::env::consts::ARCH {
        "aarch64" => "aarch64",
        "x86_64" => "x86_64",
        other => bail!("no release is published for this architecture ({other})"),
    };
    let (os, ext) = match std::env::consts::OS {
        "macos" => ("macos", "tar.gz"),
        "linux" => ("linux", "tar.gz"),
        "windows" => ("windows", "zip"),
        other => bail!("no release is published for this platform ({other})"),
    };
    Ok(format!("telemaco-{arch}-{os}{}.{ext}", variant_suffix()))
}

/// Parse `1.2.3` into something comparable. Anything unparsable sorts lowest,
/// so a malformed remote tag never masquerades as an upgrade.
fn parse_version(raw: &str) -> (u64, u64, u64) {
    let mut parts = raw.trim().trim_start_matches('v').split('.');
    let mut next = || parts.next().and_then(|p| {
        p.split(|c: char| !c.is_ascii_digit()).next().unwrap_or("").parse().ok()
    }).unwrap_or(0);
    (next(), next(), next())
}

async fn latest_release_tag(client: &reqwest::Client) -> Result<String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    // Read as text and parse with serde_json rather than reqwest's `json`
    // helper: that helper needs a reqwest feature the workspace does not enable,
    // and turning it on would change the build for every crate that shares it.
    let raw = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("asking GitHub for the latest release")?
        .error_for_status()
        .context("GitHub refused the release query")?
        .text()
        .await
        .context("reading the release response")?;
    let body: serde_json::Value =
        serde_json::from_str(&raw).context("the release response was not JSON")?;
    body.get("tag_name")
        .and_then(|t| t.as_str())
        .map(|t| t.to_string())
        .context("the release response carried no tag_name")
}

/// Where the running binary lives, and where its worker should sit.
fn install_paths() -> Result<(PathBuf, PathBuf)> {
    let exe = std::env::current_exe().context("locating the running binary")?;
    let exe = exe.canonicalize().unwrap_or(exe);
    let dir = exe.parent().context("the binary has no parent directory")?.to_path_buf();
    let worker = dir.join(if cfg!(windows) { "telemaco-worker.exe" } else { "telemaco-worker" });
    Ok((exe, worker))
}

/// Refuse rather than half-succeed: a partially replaced install is worse than
/// no update, and the caller can always fix a permission problem and retry.
fn check_writable(path: &Path) -> Result<()> {
    let dir = path.parent().context("no parent directory")?;
    let probe = dir.join(".telemaco-update-probe");
    std::fs::write(&probe, b"")
        .with_context(|| format!("{} is not writable; re-run with the rights to replace it", dir.display()))?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

pub async fn run(check_only: bool, force: bool) -> Result<()> {
    let current = env!("TELEMACO_BUILD_VERSION");
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .context("building the HTTP client")?;

    let tag = latest_release_tag(&client).await?;
    let latest = tag.trim_start_matches('v').to_string();

    let up_to_date = parse_version(&latest) <= parse_version(current);
    println!("  installed: {current}");
    println!("  latest:    {latest}");

    if check_only {
        if up_to_date {
            println!("  up to date");
            return Ok(());
        }
        println!("  an update is available: run `telemaco update`");
        // Non-zero so a script can branch on it without parsing text.
        std::process::exit(1);
    }

    if up_to_date && !force {
        println!("  nothing to do");
        return Ok(());
    }

    if cfg!(windows) {
        bail!(
            "self-update is not supported on Windows yet: a running .exe cannot be \
             replaced in place. Download {} from \
             https://github.com/{REPO}/releases/latest and unpack it over your install.",
            asset_name()?
        );
    }

    let (exe, worker) = install_paths()?;
    check_writable(&exe)?;

    let asset = asset_name()?;
    println!("  downloading {asset}");
    let url = format!("https://github.com/{REPO}/releases/download/{tag}/{asset}");
    let bytes = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("downloading {url}"))?
        .error_for_status()
        .with_context(|| format!("the release has no asset named {asset}"))?
        .bytes()
        .await
        .context("reading the downloaded archive")?;

    // Unpack beside the target so the final rename stays on one filesystem;
    // a rename across devices fails, and a copy is not atomic.
    let staging = exe.parent().unwrap().join(".telemaco-update");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).context("creating the staging directory")?;
    let archive = staging.join(&asset);
    std::fs::write(&archive, &bytes).context("writing the archive")?;

    let status = std::process::Command::new("tar")
        .arg("xzf")
        .arg(&archive)
        .arg("-C")
        .arg(&staging)
        .status()
        .context("running tar to unpack the archive")?;
    if !status.success() {
        let _ = std::fs::remove_dir_all(&staging);
        bail!("tar could not unpack {asset}");
    }

    let new_exe = staging.join("telemaco");
    if !new_exe.is_file() {
        let _ = std::fs::remove_dir_all(&staging);
        bail!("the archive did not contain a telemaco binary");
    }

    // Run the downloaded binary before trusting it: a truncated or wrong-arch
    // download would otherwise replace a working install with a broken one.
    let probe = std::process::Command::new(&new_exe)
        .arg("--version")
        .output()
        .context("the downloaded binary would not execute")?;
    let reported = String::from_utf8_lossy(&probe.stdout);
    if !probe.status.success() || !reported.contains(&latest) {
        let _ = std::fs::remove_dir_all(&staging);
        bail!("the downloaded binary reports {:?}, expected {latest}", reported.trim());
    }

    // The worker goes first: if it fails, the old telemaco is still in place and
    // still matches the old worker.
    let new_worker = staging.join("telemaco-worker");
    if new_worker.is_file() {
        std::fs::rename(&new_worker, &worker)
            .with_context(|| format!("replacing {}", worker.display()))?;
    }
    std::fs::rename(&new_exe, &exe)
        .with_context(|| format!("replacing {}", exe.display()))?;

    let _ = std::fs::remove_dir_all(&staging);
    println!("  updated {current} -> {latest}");
    println!("  {}", exe.display());
    Ok(())
}

// ---------------------------------------------------------------- passive hint

/// How long a check result stands before another one is worth making.
const CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);
/// A slow or unreachable GitHub must never hold up the command that already ran.
const CHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

fn cache_file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("telemaco").join("last-update-check"))
}

fn check_is_due(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return true;
    };
    meta.modified()
        .ok()
        .and_then(|m| m.elapsed().ok())
        .map(|age| age >= CHECK_INTERVAL)
        .unwrap_or(true)
}

/// Tell an interactive user that a newer release exists. Never for an agent.
///
/// Deliberately narrow, because a browser people reach for to avoid leaving
/// traces should not phone home on its own:
///
/// - only when stderr is a terminal, so a script, a CI job, or an MCP client
///   (whose stderr is a pipe) triggers nothing and sees nothing;
/// - at most once a day, remembered in the cache file;
/// - `TELEMACO_NO_UPDATE_CHECK=1` switches it off entirely;
/// - bounded by a short timeout, and silent on every failure: an offline or
///   firewalled machine must not pay for this.
///
/// Called after the command has finished, so it can only ever add time to a
/// run that is already over.
pub async fn maybe_notify() {
    if std::env::var_os("TELEMACO_NO_UPDATE_CHECK").is_some() {
        return;
    }
    if !std::io::IsTerminal::is_terminal(&std::io::stderr()) {
        return;
    }
    let Some(cache) = cache_file() else { return };
    if !check_is_due(&cache) {
        return;
    }

    // Stamp the cache before asking, not after: a machine that cannot reach
    // GitHub would otherwise retry on every single run.
    if let Some(dir) = cache.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&cache, b"");

    let Ok(client) = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(CHECK_TIMEOUT)
        .build()
    else {
        return;
    };
    let Ok(Ok(tag)) = tokio::time::timeout(CHECK_TIMEOUT, latest_release_tag(&client)).await else {
        return;
    };

    let latest = tag.trim_start_matches('v');
    let current = env!("TELEMACO_BUILD_VERSION");
    if parse_version(latest) > parse_version(current) {
        eprintln!(
            "\ntelemaco {latest} is available (you have {current}). \
             Run `telemaco update`, or set TELEMACO_NO_UPDATE_CHECK=1 to stop checking."
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_component_not_by_string() {
        // "0.1.10" < "0.1.9" as strings, which is the classic way this goes wrong.
        assert!(parse_version("0.1.10") > parse_version("0.1.9"));
        assert!(parse_version("0.2.0") > parse_version("0.1.99"));
        assert!(parse_version("1.0.0") > parse_version("0.99.99"));
        assert_eq!(parse_version("0.1.2"), parse_version("v0.1.2"));
    }

    #[test]
    fn unparsable_versions_never_look_like_an_upgrade() {
        // A malformed or unexpected remote tag must not trigger an update.
        let current = parse_version("0.1.2");
        for junk in ["", "latest", "nightly", "v", "not-a-version"] {
            assert!(
                parse_version(junk) <= current,
                "{junk:?} was treated as newer than 0.1.2"
            );
        }
    }

    #[test]
    fn prerelease_suffixes_do_not_break_the_patch_number() {
        assert_eq!(parse_version("0.1.2-rc1"), (0, 1, 2));
        assert_eq!(parse_version("0.1.2+build7"), (0, 1, 2));
    }

    #[test]
    fn every_variant_maps_to_the_archive_the_release_publishes() {
        // The four names here are the suffixes release.yml actually uploads.
        // Getting one wrong is a silent downgrade rather than an error.
        assert_eq!(variant_suffix_for(true, true), "-stealth");
        assert_eq!(variant_suffix_for(true, false), "");
        assert_eq!(variant_suffix_for(false, true), "-no-render-stealth");
        assert_eq!(variant_suffix_for(false, false), "-no-render");
    }

    #[test]
    fn the_asset_matches_how_this_binary_was_built() {
        // Downloading the wrong variant is a silent downgrade: a stealth user
        // would be handed the plain build and lose the transport with no error.
        let name = asset_name().expect("this platform publishes releases");
        assert_eq!(name.contains("-stealth"), cfg!(feature = "stealth"));
        assert_eq!(name.contains("-no-render"), !cfg!(feature = "render"));
        assert!(name.starts_with("telemaco-"));
    }
}
