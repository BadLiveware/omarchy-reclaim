use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

use crate::model::{Candidate, Kind, Removal, Verdict};
use crate::plan::Context;
use crate::text::{counted, sample};
use crate::{activity, command, github, scan};

const BRANCH_PREFIX: &str = "refs/heads/";
const GITDIR_PREFIX: &str = "gitdir:";
/// Treehouse removes its own pool worktrees with `treehouse prune`.
const TREEHOUSE_DIR: &str = ".treehouse";
const IGNORED_PREFIX: &str = "!! ";
const GITMODULES: &str = ".gitmodules";
const BLOCKER_SEPARATOR: &str = "; ";
const STASH_REF: &str = "--glob=refs/stash";

#[derive(Debug, Default)]
struct Linked {
    path: PathBuf,
    head: Option<String>,
    branch: Option<String>,
    locked: bool,
    prunable: bool,
}

enum Landing {
    Landed(String),
    Unlanded(String),
}

pub fn candidates(repo: &Path, ctx: &Context) -> Result<Vec<Candidate>> {
    let listing = command::git(repo, &["worktree", "list", "--porcelain"])?;
    Ok(parse(&listing)
        .into_iter()
        .filter(|linked| !linked.prunable && !is_treehouse(&linked.path))
        .map(|linked| judge(repo, linked, ctx))
        .collect())
}

/// Git refuses to remove a worktree with submodules unless forced. Forcing also deletes the
/// submodules' git directories, which live in the worktree's admin directory. `landing` has
/// checked every submodule's files, branches and stash by then, so forcing loses nothing.
pub fn remove(repo: &Path, worktree: &Path) -> Result<()> {
    let path = worktree.to_string_lossy();
    let mut args = vec!["worktree", "remove"];
    if has_submodules(worktree) {
        args.push("--force");
    }
    args.push(&path);
    command::git(repo, &args)?;
    Ok(())
}

/// Removes a worktree even with uncommitted or ignored files. Only for a person's explicit decision.
pub fn force_remove(repo: &Path, worktree: &Path) -> Result<()> {
    let path = worktree.to_string_lossy();
    command::git(repo, &["worktree", "remove", "--force", &path])?;
    Ok(())
}

/// The main working tree that a linked worktree belongs to.
pub fn main_repo(worktree: &Path) -> Result<PathBuf> {
    let common = command::git(
        worktree,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let common = PathBuf::from(common.trim());
    common
        .parent()
        .map(Path::to_path_buf)
        .with_context(|| format!("no repository above {}", common.display()))
}

/// The first block is the main working tree, which is never a candidate.
fn parse(porcelain: &str) -> Vec<Linked> {
    porcelain
        .split("\n\n")
        .filter(|block| !block.trim().is_empty())
        .skip(1)
        .map(parse_block)
        .collect()
}

fn parse_block(block: &str) -> Linked {
    let mut linked = Linked::default();
    for line in block.lines() {
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        match key {
            "worktree" => linked.path = PathBuf::from(value),
            "HEAD" => linked.head = Some(value.to_owned()),
            "branch" => {
                linked.branch = Some(
                    value
                        .strip_prefix(BRANCH_PREFIX)
                        .unwrap_or(value)
                        .to_owned(),
                )
            }
            "locked" => linked.locked = true,
            "prunable" => linked.prunable = true,
            _ => {}
        }
    }
    linked
}

fn is_treehouse(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == TREEHOUSE_DIR)
}

fn judge(repo: &Path, linked: Linked, ctx: &Context) -> Candidate {
    let idle_days = activity::idle_days(last_activity(&linked.path), ctx.now);
    let verdict = verdict(&linked, idle_days, ctx);
    Candidate {
        kind: Kind::Worktree,
        path: linked.path,
        idle_days,
        verdict,
        removal: Removal::GitWorktree {
            repo: repo.to_path_buf(),
        },
    }
}

fn verdict(linked: &Linked, idle_days: u64, ctx: &Context) -> Verdict {
    if linked.locked {
        return Verdict::Keep("locked".to_owned());
    }
    if let Some(process) = ctx.processes.using(&linked.path) {
        return Verdict::Keep(format!("in use by {} (pid {})", process.name, process.pid));
    }
    if idle_days < ctx.config.worktree_idle_days {
        return Verdict::Keep(format!("used {idle_days} days ago"));
    }
    match landing(linked, &ctx.config.regenerable_ignored) {
        Ok(Landing::Landed(reason)) => Verdict::Delete(reason),
        Ok(Landing::Unlanded(reason)) => stale(reason, idle_days, ctx),
        Err(error) => stale(format!("could not check: {error:#}"), idle_days, ctx),
    }
}

fn stale(reason: String, idle_days: u64, ctx: &Context) -> Verdict {
    if idle_days < ctx.config.needs_decision_idle_days {
        return Verdict::Keep(reason);
    }
    Verdict::NeedsDecision(reason)
}

/// `git worktree remove` deletes ignored files too, so they must all be rebuildable.
/// Submodules hold their own work, so each one is checked the same way.
/// Every blocker is listed, so a decision shown to a person covers everything that would be lost.
fn landing(linked: &Linked, regenerable: &[String]) -> Result<Landing> {
    let mut blockers = local_only_work(&linked.path, regenerable)?;
    for submodule in submodules(&linked.path)? {
        let name = submodule
            .strip_prefix(&linked.path)
            .unwrap_or(&submodule)
            .display()
            .to_string();
        let mut found = local_only_work(&submodule, regenerable)?;
        let unpushed = unpushed_on_any_branch(&submodule)?;
        if unpushed > 0 {
            found.push(format!("{} on no remote", counted(unpushed, "commit")));
        }
        blockers.extend(
            found
                .into_iter()
                .map(|blocker| format!("{name}: {blocker}")),
        );
    }
    match commits(linked)? {
        Landing::Landed(reason) if blockers.is_empty() => Ok(Landing::Landed(reason)),
        Landing::Landed(_) => Ok(Landing::Unlanded(blockers.join(BLOCKER_SEPARATOR))),
        Landing::Unlanded(blocker) => {
            blockers.push(blocker);
            Ok(Landing::Unlanded(blockers.join(BLOCKER_SEPARATOR)))
        }
    }
}

/// Whether every commit exists somewhere else: on a remote, or as a finished pull request.
fn commits(linked: &Linked) -> Result<Landing> {
    let unpushed = unpushed(&linked.path)?;
    if unpushed == 0 {
        return Ok(Landing::Landed("every commit is on a remote".to_owned()));
    }
    let unlanded = format!("{} on no remote", counted(unpushed, "commit"));
    let (Some(branch), Some(head)) = (&linked.branch, &linked.head) else {
        return Ok(Landing::Unlanded(unlanded));
    };
    match github::finished_pull_request(&linked.path, branch, head) {
        Ok(Some(pull)) => Ok(Landing::Landed(format!(
            "pull request #{} {} at this commit",
            pull.number,
            pull.state.to_lowercase()
        ))),
        Ok(None) => Ok(Landing::Unlanded(unlanded)),
        Err(_) => Ok(Landing::Unlanded(format!(
            "{unlanded}, no pull request found"
        ))),
    }
}

/// Uncommitted changes, and ignored files that cannot be rebuilt: work that exists only in `dir`.
fn local_only_work(dir: &Path, regenerable: &[String]) -> Result<Vec<String>> {
    let status = command::git(
        dir,
        &[
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--ignored=matching",
        ],
    )?;
    let (ignored, changes): (Vec<&str>, Vec<&str>) = status
        .lines()
        .partition(|line| line.starts_with(IGNORED_PREFIX));
    let mut blockers = Vec::new();
    if !changes.is_empty() {
        blockers.push(counted(changes.len() as u64, "uncommitted change"));
    }
    let precious: Vec<&str> = ignored
        .iter()
        .map(|line| &line[IGNORED_PREFIX.len()..])
        .filter(|path| !is_regenerable(dir, path, regenerable))
        .collect();
    if !precious.is_empty() {
        blockers.push(format!("ignored files: {}", sample(&precious)));
    }
    Ok(blockers)
}

/// The branch survives in the main repository when its worktree is removed, so only HEAD counts.
fn unpushed(dir: &Path) -> Result<u64> {
    count_unpushed(dir, &["HEAD"])
}

/// A submodule's branches and stash are deleted with the worktree that holds them.
fn unpushed_on_any_branch(dir: &Path) -> Result<u64> {
    count_unpushed(dir, &["HEAD", "--branches", STASH_REF])
}

fn count_unpushed(dir: &Path, refs: &[&str]) -> Result<u64> {
    let mut args = vec!["rev-list", "--count"];
    args.extend(refs);
    args.extend(["--not", "--remotes"]);
    Ok(command::git(dir, &args)?.trim().parse()?)
}

/// Checked-out submodules, nested ones included. Submodules that were never initialised hold no files.
fn submodules(worktree: &Path) -> Result<Vec<PathBuf>> {
    if !has_submodules(worktree) {
        return Ok(Vec::new());
    }
    let listing = command::git(
        worktree,
        &["submodule", "foreach", "--quiet", "--recursive", "pwd"],
    )?;
    Ok(listing.lines().map(PathBuf::from).collect())
}

fn has_submodules(worktree: &Path) -> bool {
    worktree.join(GITMODULES).is_file()
}

fn is_regenerable(worktree: &Path, ignored: &str, regenerable: &[String]) -> bool {
    let relative = ignored.trim_end_matches('/');
    let name = relative.rsplit('/').next().unwrap_or(relative);
    regenerable.iter().any(|known| known == name) || scan::is_cache_dir(&worktree.join(relative))
}

/// Git rewrites the worktree's HEAD, index or reflog on almost every command, so their times show recent use.
fn last_activity(worktree: &Path) -> Option<std::time::SystemTime> {
    let mut paths = vec![worktree.to_path_buf()];
    if let Some(admin) = admin_dir(worktree) {
        paths.extend(["HEAD", "index", "logs/HEAD"].map(|name| admin.join(name)));
    }
    activity::newest_of(&paths)
}

fn admin_dir(worktree: &Path) -> Option<PathBuf> {
    let pointer = fs::read_to_string(worktree.join(".git")).ok()?;
    let dir = PathBuf::from(pointer.strip_prefix(GITDIR_PREFIX)?.trim());
    if dir.is_relative() {
        return Some(worktree.join(dir));
    }
    Some(dir)
}
