//! Files in the state directory that describe the last scan and the last run.
//! The Omarchy bar widget reads them.

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::{self, Config};
use crate::disk::Usage;
use crate::model::{Kind, Verdict};
use crate::plan::Measured;
use crate::state;

pub const PLAN_FILE: &str = "plan.json";
pub const LAST_RUN_FILE: &str = "last-run.json";

#[derive(Serialize)]
pub struct KindTotal {
    pub kind: &'static str,
    pub count: usize,
    pub bytes: u64,
}

#[derive(Serialize)]
pub struct Decision {
    pub kind: &'static str,
    pub path: String,
    pub display: String,
    pub reason: String,
    pub idle_days: u64,
    pub bytes: u64,
}

#[derive(Serialize)]
pub struct DiskSnapshot {
    pub free_bytes: u64,
    pub total_bytes: u64,
}

impl From<&Usage> for DiskSnapshot {
    fn from(usage: &Usage) -> Self {
        Self {
            free_bytes: usage.free,
            total_bytes: usage.total,
        }
    }
}

/// What a run would do now: deletions still pending, worktrees that need a decision.
#[derive(Serialize)]
pub struct PlanSnapshot {
    pub scanned_at: u64,
    pub low_disk_free_percent: u8,
    pub reclaimable_bytes: u64,
    pub reclaimable: Vec<KindTotal>,
    pub needs_decision: Vec<Decision>,
    pub kept: usize,
}

impl PlanSnapshot {
    pub fn new(config: &Config, measured: &[Measured]) -> Result<Self> {
        let roots = config.root_paths()?;
        let mut totals: BTreeMap<Kind, (usize, u64)> = BTreeMap::new();
        let mut needs_decision = Vec::new();
        let mut kept = 0;
        for item in measured {
            let candidate = &item.candidate;
            let bytes = item.bytes.unwrap_or_default();
            match &candidate.verdict {
                Verdict::Delete(_) => {
                    let total = totals.entry(candidate.kind).or_default();
                    total.0 += 1;
                    total.1 += bytes;
                }
                Verdict::NeedsDecision(reason) => needs_decision.push(Decision {
                    kind: candidate.kind.label(),
                    path: candidate.path.display().to_string(),
                    display: short_path(&candidate.path, &roots),
                    reason: reason.clone(),
                    idle_days: candidate.idle_days,
                    bytes,
                }),
                Verdict::Keep(_) => kept += 1,
            }
        }
        needs_decision.sort_by_key(|decision| std::cmp::Reverse(decision.bytes));
        Ok(Self {
            scanned_at: state::unix_now(),
            low_disk_free_percent: config.low_disk_free_percent,
            reclaimable_bytes: totals.values().map(|(_, bytes)| bytes).sum(),
            reclaimable: kind_totals(&totals),
            needs_decision,
            kept,
        })
    }
}

/// Relative to the scan root that holds it, for a narrow panel. Paths outside the roots keep `~/`.
fn short_path(path: &Path, roots: &[PathBuf]) -> String {
    roots
        .iter()
        .find_map(|root| path.strip_prefix(root).ok())
        .map(|relative| relative.display().to_string())
        .unwrap_or_else(|| config::display(path))
}

#[derive(Serialize)]
pub struct RunSnapshot {
    pub finished_at: u64,
    pub low_disk_only: bool,
    pub freed_bytes: u64,
    pub deleted: Vec<KindTotal>,
    pub failed: usize,
    pub docker: Vec<String>,
    pub disk: DiskSnapshot,
}

pub fn kind_totals(totals: &BTreeMap<Kind, (usize, u64)>) -> Vec<KindTotal> {
    let mut list: Vec<KindTotal> = totals
        .iter()
        .map(|(kind, (count, bytes))| KindTotal {
            kind: kind.label(),
            count: *count,
            bytes: *bytes,
        })
        .collect();
    list.sort_by_key(|total| std::cmp::Reverse(total.bytes));
    list
}

/// The fields of `last-run.json` that the next run needs.
#[derive(Deserialize)]
pub struct LastRun {
    pub finished_at: u64,
    pub disk: LastRunDisk,
}

#[derive(Deserialize)]
pub struct LastRunDisk {
    pub free_bytes: u64,
}

pub fn read_last_run(dir: &Path) -> Result<Option<LastRun>> {
    let path = dir.join(LAST_RUN_FILE);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };
    let last =
        serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))?;
    Ok(Some(last))
}

/// Drops one decision from `plan.json` after a person resolved it, so readers update at once.
pub fn forget_decision(dir: &Path, path: &Path) -> Result<()> {
    let file = dir.join(PLAN_FILE);
    let bytes = match fs::read(&file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).with_context(|| format!("reading {}", file.display())),
    };
    let mut plan: serde_json::Value = serde_json::from_slice(&bytes)?;
    let target = path.display().to_string();
    if let Some(decisions) = plan["needs_decision"].as_array_mut() {
        decisions.retain(|decision| decision["path"] != target.as_str());
    }
    state::write_json(dir, PLAN_FILE, &plan)
}
