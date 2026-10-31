//! Thin async wrapper around the `nix` CLI. Everything shells out; we never
//! link against Nix itself.

pub mod profile;
pub mod rank;
pub mod search;

use std::process::Stdio;

use anyhow::{bail, Context, Result};
use tokio::process::Command;

pub(crate) struct Run {
    pub stdout: String,
    #[allow(dead_code)]
    pub stderr: String,
}

/// Run `nix <args>` and fail with the tail of stderr on a non-zero exit.
///
/// The child is killed if the returned future is dropped (or its task aborted),
/// which is how a superseded search gets cancelled.
pub(crate) async fn run(args: &[&str]) -> Result<Run> {
    let out = Command::new("nix")
        .args(["--extra-experimental-features", "nix-command flakes"])
        .args(args)
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .context("could not run `nix` (is it in your PATH?)")?;

    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    if !out.status.success() {
        bail!("{}", tail(&stderr, 8));
    }
    Ok(Run { stdout, stderr })
}

fn tail(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}
