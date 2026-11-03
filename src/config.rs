use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

const DEFAULT_ROOT: &str = "~/code";
const DEFAULT_BUILD_OUTPUT_IDLE_DAYS: u64 = 14;
const DEFAULT_WORKTREE_IDLE_DAYS: u64 = 7;
const DEFAULT_NEEDS_DECISION_IDLE_DAYS: u64 = 30;
const DEFAULT_LOW_DISK_FREE_PERCENT: u8 = 15;
const DEFAULT_LOW_DISK_NOTIFY_EVERY_HOURS: u64 = 6;
const DEFAULT_LOW_DISK_RERUN_AFTER_HOURS: u64 = 6;
const DEFAULT_LOW_DISK_RERUN_DROP_PERCENT: u8 = 1;
const DEFAULT_DOCKER_IMAGE_IDLE_HOURS: u64 = 168;
const DEFAULT_REGENERABLE_IGNORED: [&str; 22] = [
    ".cache",
    ".gradle",
    ".next",
    ".pytest_cache",
    ".svelte-kit",
    ".terraform",
    ".terraform.lock.hcl",
    ".terragrunt-cache",
    ".turbo",
    ".venv",
    "__pycache__",
    "bin",
    "build",
    "coverage",
    "dist",
    "node_modules",
    "obj",
    "out",
    "target",
    "test-results",
    "TestResults",
    "tsconfig.tsbuildinfo",
];

#[derive(Debug, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Directories that are scanned for build outputs and git repositories.
    pub roots: Vec<PathBuf>,
    /// Build outputs not touched for this many days are deleted.
    pub build_output_idle_days: u64,
    /// Worktrees with all work landed and idle for this many days are deleted.
    pub worktree_idle_days: u64,
    /// Worktrees with unlanded work idle for this many days are reported as needing a decision.
    pub needs_decision_idle_days: u64,
    /// Names of git-ignored files and directories that can be rebuilt.
    /// A worktree holding any other ignored path (`.env`, notes, agent state) is never deleted.
    /// Ignored directories with a `CACHEDIR.TAG` also count as rebuildable.
    pub regenerable_ignored: Vec<String>,
    /// `run --only-if-low-disk` does nothing while more space than this is free.
    pub low_disk_free_percent: u8,
    /// Minimum time between two "disk is still low" notifications.
    pub low_disk_notify_every_hours: u64,
    /// While space stays low, the low-disk check runs again only after this long...
    pub low_disk_rerun_after_hours: u64,
    /// ...or once free space has fallen by this much of the disk since the last run.
    pub low_disk_rerun_drop_percent: u8,
    pub docker: bool,
    /// Unused Docker images and build cache older than this are deleted. Unused volumes are always deleted.
    pub docker_image_idle_hours: u64,
    pub treehouse: bool,
    pub notify: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            roots: vec![PathBuf::from(DEFAULT_ROOT)],
            build_output_idle_days: DEFAULT_BUILD_OUTPUT_IDLE_DAYS,
            worktree_idle_days: DEFAULT_WORKTREE_IDLE_DAYS,
            needs_decision_idle_days: DEFAULT_NEEDS_DECISION_IDLE_DAYS,
            regenerable_ignored: DEFAULT_REGENERABLE_IGNORED.map(String::from).to_vec(),
            low_disk_free_percent: DEFAULT_LOW_DISK_FREE_PERCENT,
            low_disk_notify_every_hours: DEFAULT_LOW_DISK_NOTIFY_EVERY_HOURS,
            low_disk_rerun_after_hours: DEFAULT_LOW_DISK_RERUN_AFTER_HOURS,
            low_disk_rerun_drop_percent: DEFAULT_LOW_DISK_RERUN_DROP_PERCENT,
            docker: true,
            docker_image_idle_hours: DEFAULT_DOCKER_IMAGE_IDLE_HOURS,
            treehouse: true,
            notify: true,
        }
    }
}

impl Config {
    pub fn load(explicit: Option<&Path>) -> Result<Self> {
        let path = match explicit {
            Some(path) => path.to_path_buf(),
            None => default_path()?,
        };
        if explicit.is_none() && !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading config {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing config {}", path.display()))
    }

    pub fn root_paths(&self) -> Result<Vec<PathBuf>> {
        self.roots.iter().map(|root| expand_home(root)).collect()
    }
}

pub fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")
}

fn default_path() -> Result<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => home()?.join(".config"),
    };
    Ok(base.join(env!("CARGO_PKG_NAME")).join("config.toml"))
}

fn expand_home(path: &Path) -> Result<PathBuf> {
    match path.strip_prefix("~") {
        Ok(rest) => Ok(home()?.join(rest)),
        Err(_) => Ok(path.to_path_buf()),
    }
}

/// Shows a path with the home directory as `~`, for humans.
pub fn display(path: &Path) -> String {
    let Ok(home) = home() else {
        return path.display().to_string();
    };
    match path.strip_prefix(&home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_config_matches_defaults() {
        let example: Config = toml::from_str(include_str!("../config.example.toml")).unwrap();
        assert_eq!(example, Config::default());
    }
}
