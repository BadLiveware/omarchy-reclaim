use std::path::Path;
use std::time::SystemTime;

use crate::activity;
use crate::model::{Candidate, Removal, Verdict};
use crate::plan::Context;
use crate::scan::BuildOutput;

/// Deep enough to see `target/debug/deps`, `node_modules/<package>/package.json` and `bin/Debug/<tfm>`.
const OUTPUT_DEPTH: usize = 2;
/// Deep enough to see edits in `src/` and the project files next to the output.
const PROJECT_DEPTH: usize = 2;

pub fn judge(output: BuildOutput, ctx: &Context) -> Candidate {
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
