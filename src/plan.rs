use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::Result;
use rayon::prelude::*;

use crate::config::{self, Config};
use crate::disk;
use crate::model::{Candidate, Verdict};
use crate::procs::Processes;
use crate::{outputs, scan, worktrees};

pub struct Context<'a> {
    pub config: &'a Config,
    pub processes: Processes,
    pub now: SystemTime,
}

pub struct Plan {
    pub candidates: Vec<Candidate>,
}

impl Plan {
    pub fn build(config: &Config) -> Result<Self> {
        let ctx = Context {
            config,
            processes: Processes::snapshot(),
            now: SystemTime::now(),
        };
        let scan = scan::scan(&config.root_paths()?);
        let mut candidates = Vec::new();
        for repo in &scan.repos {
            match worktrees::candidates(repo, &ctx) {
                Ok(found) => candidates.extend(found),
                Err(error) => {
                    eprintln!("skipping worktrees of {}: {error:#}", config::display(repo))
                }
            }
        }
        let doomed: Vec<PathBuf> = candidates
            .iter()
            .filter(|candidate| matches!(candidate.verdict, Verdict::Delete(_)))
            .map(|candidate| candidate.path.clone())
            .collect();
        let outputs = scan
            .outputs
            .into_iter()
            .filter(|output| !doomed.iter().any(|dir| output.path.starts_with(dir)))
            .collect();
        candidates.extend(outputs::judge_all(outputs, &ctx));
        Ok(Self { candidates })
    }
}

pub struct Measured {
    pub candidate: Candidate,
    pub bytes: Option<u64>,
}

/// Kept items are only sized on request. They are many, and a kept worktree also holds
/// build outputs that are counted on their own.
pub fn measure(candidates: Vec<Candidate>, include_kept: bool) -> Vec<Measured> {
    candidates
        .into_par_iter()
        .map(|candidate| {
            let wanted = include_kept || !matches!(candidate.verdict, Verdict::Keep(_));
            let bytes = wanted.then(|| disk::size_on_disk(&candidate.path));
            Measured { candidate, bytes }
        })
        .collect()
}
