use std::fs;
use std::path::{Path, PathBuf};

const PROC: &str = "/proc";
const DELETED_SUFFIX: &str = " (deleted)";

/// A process and every path it holds: working directory, executable and memory-mapped files.
pub struct Process {
    pub pid: u32,
    pub name: String,
    paths: Vec<PathBuf>,
}

pub struct Processes(Vec<Process>);

impl Processes {
    /// Reads all processes this user may inspect. Other users' processes are not readable and are skipped.
    pub fn snapshot() -> Self {
        let own = std::process::id();
        let Ok(entries) = fs::read_dir(PROC) else {
            return Self(Vec::new());
        };
        let processes = entries
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
            .filter(|pid| *pid != own)
            .filter_map(read_process)
            .collect();
        Self(processes)
    }

    pub fn using(&self, path: &Path) -> Option<&Process> {
        self.0
            .iter()
            .find(|process| process.paths.iter().any(|held| held.starts_with(path)))
    }
}

fn read_process(pid: u32) -> Option<Process> {
    let base = Path::new(PROC).join(pid.to_string());
    let cwd = fs::read_link(base.join("cwd")).ok()?;
    let name = fs::read_to_string(base.join("comm"))
        .map(|name| name.trim().to_owned())
        .unwrap_or_default();
    let mut paths = vec![cwd];
    paths.extend(fs::read_link(base.join("exe")).ok());
    if let Ok(maps) = fs::read_to_string(base.join("maps")) {
        paths.extend(mapped_files(&maps));
    }
    paths.sort();
    paths.dedup();
    Some(Process { pid, name, paths })
}

/// The path is the last field of a `/proc/<pid>/maps` line and may contain spaces.
/// The fields before it never contain " /".
fn mapped_files(maps: &str) -> impl Iterator<Item = PathBuf> + '_ {
    maps.lines().filter_map(|line| {
        let (_, path) = line.split_once(" /")?;
        let path = path.strip_suffix(DELETED_SUFFIX).unwrap_or(path);
        Some(Path::new("/").join(path))
    })
}
