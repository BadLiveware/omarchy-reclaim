use std::fs::{self, File, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::config;

const LOCK_FILE: &str = "lock";
const TEMPORARY_SUFFIX: &str = ".tmp";

pub fn dir() -> Result<PathBuf> {
    let base = match std::env::var_os("XDG_STATE_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => config::home()?.join(".local/state"),
    };
    let dir = base.join(env!("CARGO_PKG_NAME"));
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok(dir)
}

/// One scan or run at a time. `None` when another holds the lock; the lock lasts as long as the file.
pub fn lock(dir: &Path) -> Result<Option<File>> {
    let file = File::create(dir.join(LOCK_FILE))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => Err(error.into()),
    }
}

/// Readers such as the bar widget watch these files, so they must never see a half-written one.
pub fn write_json(dir: &Path, name: &str, value: &impl Serialize) -> Result<()> {
    let path = dir.join(name);
    let temporary = dir.join(format!("{name}{TEMPORARY_SUFFIX}"));
    fs::write(&temporary, serde_json::to_vec_pretty(value)?)?;
    fs::rename(&temporary, &path).with_context(|| format!("writing {}", path.display()))
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}
