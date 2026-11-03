use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::Result;

use crate::model::{Candidate, Removal, Verdict};
use crate::plan::Context;
use crate::scan::BuildOutput;
use crate::text::sample;
use crate::{activity, command};

/// Deep enough to see `target/debug/deps`, `node_modules/<package>/package.json` and `bin/Debug/<tfm>`.
const OUTPUT_DEPTH: usize = 2;
/// Deep enough to see edits in `src/` and the project files next to the output.
const PROJECT_DEPTH: usize = 2;
const GIT: &str = ".git";
const NUL: char = '\0';

type GitFiles = Result<HashMap<PathBuf, Vec<String>>>;

/// Judges every output. One that would be deleted is kept when it holds files that git tracks
/// or that git sees as untracked and not ignored: deleting the directory would lose them.
pub fn judge_all(outputs: Vec<BuildOutput>, ctx: &Context) -> Vec<Candidate> {
    let mut candidates: Vec<Candidate> = outputs
        .into_iter()
        .map(|output| judge(output, ctx))
        .collect();
    for (work_tree, members) in deletable_by_work_tree(&candidates) {
        let paths: Vec<&Path> = members
            .iter()
            .map(|&index| candidates[index].path.as_path())
            .collect();
        let found = files_git_would_lose(&work_tree, &paths);
        for index in members {
            if let Some(reason) = keep_reason(&found, &candidates[index].path) {
                candidates[index].verdict = Verdict::Keep(reason);
            }
        }
    }
    candidates
}

fn judge(output: BuildOutput, ctx: &Context) -> Candidate {
    let idle_days = activity::idle_days(last_use(&output.path), ctx.now);
    let verdict = verdict(&output, idle_days, ctx);
    Candidate {
        kind: output.kind,
        path: output.path,
        idle_days,
        verdict,
        removal: Removal::Directory,
    }
}

/// An output counts as used when it was written or read, or when its project was worked on.
fn last_use(output: &Path) -> Option<SystemTime> {
    let own = activity::last_use_within(output, OUTPUT_DEPTH, None);
    let project = output
        .parent()
        .and_then(|project| activity::last_use_within(project, PROJECT_DEPTH, Some(output)));
    own.max(project)
}

fn verdict(output: &BuildOutput, idle_days: u64, ctx: &Context) -> Verdict {
    if let Some(process) = ctx.processes.using(&output.path) {
        return Verdict::Keep(format!("in use by {} (pid {})", process.name, process.pid));
    }
    if idle_days < ctx.config.build_output_idle_days {
        return Verdict::Keep(format!("used {idle_days} days ago"));
    }
    Verdict::Delete(format!("unused for {idle_days} days"))
}

/// Outputs that would be deleted, grouped by the git working tree that holds them,
/// so git lists each working tree once.
fn deletable_by_work_tree(candidates: &[Candidate]) -> BTreeMap<PathBuf, Vec<usize>> {
    let mut groups: BTreeMap<PathBuf, Vec<usize>> = BTreeMap::new();
    for (index, candidate) in candidates.iter().enumerate() {
        if !matches!(candidate.verdict, Verdict::Delete(_)) {
            continue;
        }
        if let Some(work_tree) = work_tree(&candidate.path) {
            groups.entry(work_tree).or_default().push(index);
        }
    }
    groups
}

/// The nearest directory above `path` with a `.git` directory or file.
fn work_tree(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .skip(1)
        .find(|dir| dir.join(GIT).exists())
        .map(Path::to_path_buf)
}

/// For each output, its files that git tracks or does not ignore, relative to the output.
fn files_git_would_lose(work_tree: &Path, outputs: &[&Path]) -> GitFiles {
    let listing = command::git(
        work_tree,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?;
    let wanted: HashSet<&Path> = outputs.iter().copied().collect();
    let mut found: HashMap<PathBuf, Vec<String>> = HashMap::new();
    for file in listing.split(NUL).filter(|file| !file.is_empty()) {
        let path = work_tree.join(file);
        let Some(output) = path.ancestors().find(|dir| wanted.contains(dir)) else {
            continue;
        };
        let relative = path.strip_prefix(output).unwrap_or(&path);
        found
            .entry(output.to_path_buf())
            .or_default()
            .push(relative.display().to_string());
    }
    Ok(found)
}

fn keep_reason(found: &GitFiles, output: &Path) -> Option<String> {
    let files = match found {
        Ok(found) => found.get(output)?,
        Err(error) => return Some(format!("could not check git: {error:#}")),
    };
    let names: Vec<&str> = files.iter().map(String::as_str).collect();
    Some(format!("git tracks or does not ignore {}", sample(&names)))
}
