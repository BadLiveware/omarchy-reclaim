use std::process::Command;

use anyhow::Result;

use crate::command;

/// Dry run: what treehouse would remove from its pools.
pub fn report() -> Result<String> {
    prune(false)
}

/// Treehouse removes only pool worktrees that are merged, clean, idle and not leased.
pub fn reclaim() -> Result<String> {
    prune(true)
}

fn prune(apply: bool) -> Result<String> {
    let mut treehouse = Command::new("treehouse");
    treehouse.args(["prune", "--all"]);
    if apply {
        treehouse.arg("--yes");
    }
    command::output(treehouse)
}
