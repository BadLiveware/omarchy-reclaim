use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

/// Runs a command and returns stdout. A non-zero exit becomes an error carrying stderr.
pub fn output(mut command: Command) -> Result<String> {
    let description = format!("{command:?}");
    let out = command
        .output()
        .with_context(|| format!("starting {description}"))?;
    if !out.status.success() {
        bail!(
            "{description} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Git without optional locks, so inspecting a worktree never rewrites its index or changes its activity time.
pub fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0");
    output(command)
}
