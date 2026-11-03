use std::collections::HashSet;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use anyhow::{Context, Result};
use walkdir::WalkDir;

const BLOCK_SIZE: u64 = 512;
const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
const UNIT_STEP: f64 = 1024.0;

pub struct Usage {
    pub free: u64,
    pub total: u64,
}

impl Usage {
    pub fn of(path: &Path) -> Result<Self> {
        let stats = rustix::fs::statvfs(path)
            .with_context(|| format!("reading free space of {}", path.display()))?;
        Ok(Self {
            free: stats.f_bavail * stats.f_frsize,
            total: stats.f_blocks * stats.f_frsize,
        })
    }

    pub fn free_percent(&self) -> u8 {
        if self.total == 0 {
            return 0;
        }
        u8::try_from(self.free * 100 / self.total).unwrap_or(100)
    }

    pub fn summary(&self) -> String {
        format!(
            "{} free of {} ({}% free)",
            human(self.free),
            human(self.total),
            self.free_percent()
        )
    }
}

/// Space the tree uses on disk. Hard-linked files are counted once.
pub fn size_on_disk(path: &Path) -> u64 {
    let mut seen = HashSet::new();
    WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.metadata().ok())
        .filter(|meta| meta.nlink() <= 1 || seen.insert((meta.dev(), meta.ino())))
        .map(|meta| meta.blocks() * BLOCK_SIZE)
        .sum()
}

pub fn human(bytes: u64) -> String {
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= UNIT_STEP && unit < UNITS.len() - 1 {
        value /= UNIT_STEP;
        unit += 1;
    }
    if unit == 0 {
        return format!("{bytes} B");
    }
    format!("{value:.1} {}", UNITS[unit])
}
