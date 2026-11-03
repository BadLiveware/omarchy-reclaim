use std::path::Path;
use std::process::Command;

use anyhow::Result;
use serde::Deserialize;

use crate::command;

const FINISHED_STATES: [&str; 2] = ["MERGED", "CLOSED"];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    pub number: u64,
    pub state: String,
    head_ref_oid: String,
}

/// A merged or closed pull request whose head is exactly `head`.
/// GitHub keeps such commits, so nothing in the worktree is lost when it is removed.
pub fn finished_pull_request(dir: &Path, branch: &str, head: &str) -> Result<Option<PullRequest>> {
    let mut gh = Command::new("gh");
    gh.current_dir(dir)
        .env("GH_PROMPT_DISABLED", "1")
        .args(["pr", "list", "--state", "all", "--head", branch])
        .args(["--json", "number,state,headRefOid"]);
    let pulls: Vec<PullRequest> = serde_json::from_str(&command::output(gh)?)?;
    Ok(pulls
        .into_iter()
        .find(|pull| pull.head_ref_oid == head && FINISHED_STATES.contains(&pull.state.as_str())))
}
