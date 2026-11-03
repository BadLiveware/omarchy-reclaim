use std::path::Path;
use std::time::SystemTime;

use anyhow::{Context, Result, bail};

use crate::config::{self, Config};
use crate::model::Verdict;
use crate::plan::Context as PlanContext;
use crate::procs::Processes;
use crate::snapshot;
use crate::{state, worktrees};

/// Removes a worktree that needed a decision, once a person has accepted what it holds.
/// The worktree is judged again first. It is removed only if what blocks it is still exactly
/// `accepted`, the reason the person saw, so nothing new is lost.
pub fn remove(config: &Config, path: &Path, accepted: &str) -> Result<()> {
    let dir = state::dir()?;
    let Some(_lock) = state::lock(&dir)? else {
        bail!("a scan or run is active, try again when it has finished");
    };
    let path = path
        .canonicalize()
        .with_context(|| format!("{} does not exist", path.display()))?;
    let repo = worktrees::main_repo(&path)?;
    let ctx = PlanContext {
        config,
        processes: Processes::snapshot(),
        now: SystemTime::now(),
    };
    let candidate = worktrees::candidates(&repo, &ctx)?
        .into_iter()
        .find(|candidate| {
            candidate
                .path
                .canonicalize()
                .is_ok_and(|found| found == path)
        })
        .with_context(|| format!("{} is not a linked worktree", path.display()))?;
    let current = candidate.verdict.reason();
    let unchanged = match &candidate.verdict {
        Verdict::Delete(_) => true,
        Verdict::NeedsDecision(reason) | Verdict::Keep(reason) => reason == accepted,
    };
    if !unchanged {
        bail!("not removed, it changed since the scan: {current}");
    }
    worktrees::force_remove(&repo, &path)?;
    worktrees::prune(&repo)?;
    snapshot::forget_decision(&dir, &path)?;
    println!("removed {} ({current})", config::display(&path));
    Ok(())
}
