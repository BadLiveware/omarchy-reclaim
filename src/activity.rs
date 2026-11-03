use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use walkdir::WalkDir;

const SECONDS_PER_DAY: u64 = 86_400;
const CACHEDIR_TAG: &str = "CACHEDIR.TAG";

/// Last write anywhere in the tree, or last read of a regular file, up to `depth` levels down.
/// With `relatime` the kernel records a read at most once a day, which is enough to see daily use
/// such as `npm run dev` or running a built binary. Directory access times are left out because
/// scanning lists directories, and `CACHEDIR.TAG` because the scan reads it.
/// `exclude` leaves out one subtree.
pub fn last_use_within(dir: &Path, depth: usize, exclude: Option<&Path>) -> Option<SystemTime> {
    WalkDir::new(dir)
        .max_depth(depth)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| exclude != Some(entry.path()))
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            let written = meta.modified().ok()?;
            if !meta.is_file() || entry.file_name() == CACHEDIR_TAG {
                return Some(written);
            }
            Some(meta.accessed().map_or(written, |read| read.max(written)))
        })
        .max()
}

pub fn newest_of(paths: &[PathBuf]) -> Option<SystemTime> {
    paths
        .iter()
        .filter_map(|path| fs::symlink_metadata(path).ok()?.modified().ok())
        .max()
}

/// Whole days since `last`. Unknown activity counts as active, so it is never deleted.
pub fn idle_days(last: Option<SystemTime>, now: SystemTime) -> u64 {
    let Some(last) = last else {
        return 0;
    };
    now.duration_since(last).unwrap_or(Duration::ZERO).as_secs() / SECONDS_PER_DAY
}
