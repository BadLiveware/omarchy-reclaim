use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::model::Kind;

const GIT: &str = ".git";
const NODE_MODULES: &str = "node_modules";
const PACKAGE_JSON: &str = "package.json";
const DOTNET_OUTPUTS: [&str; 2] = ["bin", "obj"];
const DOTNET_PROJECT_EXTENSIONS: [&str; 3] = ["csproj", "fsproj", "vbproj"];
/// https://bford.info/cachedir/
const CACHEDIR_TAG: &str = "CACHEDIR.TAG";
const CACHEDIR_SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";
const CARGO_TAG_MARKER: &str = "cargo";
const TAG_READ_LIMIT: u64 = 512;

pub struct BuildOutput {
    pub kind: Kind,
    pub path: PathBuf,
}

#[derive(Default)]
pub struct Scan {
    pub outputs: Vec<BuildOutput>,
    /// Main working trees of git repositories. Their linked worktrees are listed through git.
    pub repos: Vec<PathBuf>,
}

pub fn scan(roots: &[PathBuf]) -> Scan {
    let mut scan = Scan::default();
    for root in roots {
        walk(root, &mut scan);
    }
    scan
}

fn walk(root: &Path, scan: &mut Scan) {
    let mut entries = WalkDir::new(root).follow_links(false).into_iter();
    while let Some(entry) = entries.next() {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_dir() {
            continue;
        }
        let path = entry.path();
        if entry.file_name() == GIT {
            entries.skip_current_dir();
            continue;
        }
        if let Some(kind) = classify(path) {
            scan.outputs.push(BuildOutput {
                kind,
                path: path.to_path_buf(),
            });
            entries.skip_current_dir();
            continue;
        }
        if path.join(GIT).is_dir() {
            scan.repos.push(path.to_path_buf());
        }
    }
}

fn classify(dir: &Path) -> Option<Kind> {
    let name = dir.file_name()?.to_str()?;
    let parent = dir.parent()?;
    if name == NODE_MODULES && parent.join(PACKAGE_JSON).is_file() {
        return Some(Kind::Node);
    }
    if DOTNET_OUTPUTS.contains(&name) && has_dotnet_project(parent) {
        return Some(Kind::DotNet);
    }
    cache_kind(&dir.join(CACHEDIR_TAG))
}

fn has_dotnet_project(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        entry
            .path()
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| DOTNET_PROJECT_EXTENSIONS.contains(&ext))
    })
}

pub fn is_cache_dir(dir: &Path) -> bool {
    cache_kind(&dir.join(CACHEDIR_TAG)).is_some()
}

fn cache_kind(tag: &Path) -> Option<Kind> {
    let file = fs::File::open(tag).ok()?;
    let mut head = Vec::new();
    file.take(TAG_READ_LIMIT).read_to_end(&mut head).ok()?;
    if !head.starts_with(CACHEDIR_SIGNATURE) {
        return None;
    }
    if String::from_utf8_lossy(&head).contains(CARGO_TAG_MARKER) {
        return Some(Kind::Cargo);
    }
    Some(Kind::Cache)
}
