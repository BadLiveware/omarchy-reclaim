use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use anyhow::Result;

use crate::config::{self, Config};
use crate::disk::{self, Usage};
use crate::model::{Candidate, Kind, Removal, Verdict};
use crate::plan::{Measured, Plan};
use crate::snapshot::{self, PlanSnapshot, RunSnapshot};
use crate::{docker, notify, state, treehouse, worktrees};

const LOW_DISK_STAMP: &str = "low-disk-notified";
const SECONDS_PER_HOUR: u64 = 3_600;

#[derive(Default)]
struct Outcome {
    deleted: BTreeMap<Kind, (usize, u64)>,
    failed: usize,
    needs_decision: usize,
    /// What a run would still do afterwards: failed deletions, needs-decision and kept items.
    remaining: Vec<Measured>,
}

impl Outcome {
    fn freed(&self) -> u64 {
        self.deleted.values().map(|(_, bytes)| bytes).sum()
    }

    fn breakdown(&self) -> String {
        self.deleted
            .iter()
            .map(|(kind, (count, bytes))| {
                format!("{count} {} ({})", kind.label(), disk::human(*bytes))
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub fn run(config: &Config, only_if_low_disk: bool) -> Result<()> {
    let dir = state::dir()?;
    let Some(_lock) = state::lock(&dir)? else {
        println!("another scan or run is active");
        return Ok(());
    };

    let home = config::home()?;
    let before = Usage::of(&home)?;
    if only_if_low_disk {
        if before.free_percent() >= config.low_disk_free_percent {
            return Ok(());
        }
        if let Some(reason) = rerun_blocker(config, &dir, &before)? {
            println!("disk is low, but not scanning again: {reason}");
            return Ok(());
        }
    }
    println!("disk before: {}", before.summary());

    let mut outcome = Outcome::default();
    for candidate in Plan::build(config)?.candidates {
        apply(candidate, &mut outcome);
    }
    if config.treehouse {
        match treehouse::reclaim() {
            Ok(output) => output
                .lines()
                .for_each(|line| println!("treehouse: {line}")),
            Err(error) => eprintln!("treehouse: {error:#}"),
        }
    }
    let mut docker = Vec::new();
    if config.docker {
        docker = docker::reclaim(config.docker_image_idle_hours);
    }
    docker.iter().for_each(|line| println!("docker {line}"));

    let after = Usage::of(&home)?;
    println!("disk after: {}", after.summary());
    state::write_json(
        &dir,
        snapshot::PLAN_FILE,
        &PlanSnapshot::new(config, &outcome.remaining)?,
    )?;
    state::write_json(
        &dir,
        snapshot::LAST_RUN_FILE,
        &RunSnapshot {
            finished_at: state::unix_now(),
            low_disk_only: only_if_low_disk,
            freed_bytes: outcome.freed(),
            deleted: snapshot::kind_totals(&outcome.deleted),
            failed: outcome.failed,
            docker: docker.clone(),
            disk: (&after).into(),
        },
    )?;
    if config.notify {
        report(config, &outcome, &after, &docker, only_if_low_disk, &dir);
    }
    Ok(())
}

/// When a run could not lift free space above the threshold, scanning again soon finds nothing new
/// and only loads the disk. The next low-disk run waits a while, or until free space falls further.
fn rerun_blocker(config: &Config, dir: &Path, now: &Usage) -> Result<Option<String>> {
    let Some(last) = snapshot::read_last_run(dir)? else {
        return Ok(None);
    };
    let hours_since = state::unix_now().saturating_sub(last.finished_at) / SECONDS_PER_HOUR;
    if hours_since >= config.low_disk_rerun_after_hours {
        return Ok(None);
    }
    let fallen = last.disk.free_bytes.saturating_sub(now.free);
    let enough = now.total / 100 * u64::from(config.low_disk_rerun_drop_percent);
    if fallen >= enough {
        return Ok(None);
    }
    Ok(Some(format!(
        "the last run was {hours_since} h ago and free space fell only {} since",
        disk::human(fallen)
    )))
}

fn apply(candidate: Candidate, outcome: &mut Outcome) {
    let shown = config::display(&candidate.path);
    match &candidate.verdict {
        Verdict::Keep(_) => outcome.remaining.push(Measured {
            candidate,
            bytes: None,
        }),
        Verdict::NeedsDecision(reason) => {
            println!("needs a decision: {shown} ({reason})");
            outcome.needs_decision += 1;
            let bytes = Some(disk::size_on_disk(&candidate.path));
            outcome.remaining.push(Measured { candidate, bytes });
        }
        Verdict::Delete(reason) => {
            let bytes = disk::size_on_disk(&candidate.path);
            match remove(&candidate) {
                Ok(()) => {
                    println!(
                        "deleted {} {shown}, {} ({reason})",
                        candidate.kind.label(),
                        disk::human(bytes)
                    );
                    let entry = outcome.deleted.entry(candidate.kind).or_default();
                    entry.0 += 1;
                    entry.1 += bytes;
                }
                Err(error) => {
                    eprintln!("could not delete {shown}: {error:#}");
                    outcome.failed += 1;
                    outcome.remaining.push(Measured {
                        candidate,
                        bytes: Some(bytes),
                    });
                }
            }
        }
    }
}

fn remove(candidate: &Candidate) -> Result<()> {
    match &candidate.removal {
        Removal::Directory => Ok(fs::remove_dir_all(&candidate.path)?),
        Removal::GitWorktree { repo } => worktrees::remove(repo, &candidate.path),
    }
}

/// The daily run reports everything. The frequent low-disk run stays quiet unless it freed space,
/// and repeats "disk is still low" only every `low_disk_notify_every_hours`.
fn report(
    config: &Config,
    outcome: &Outcome,
    after: &Usage,
    docker: &[String],
    only_if_low_disk: bool,
    state: &Path,
) {
    let freed = outcome.freed();
    let still_low = after.free_percent() < config.low_disk_free_percent;
    let stamp = state.join(LOW_DISK_STAMP);
    let warn_low = still_low && due(&stamp, config.low_disk_notify_every_hours);
    let needs_decision = outcome.needs_decision > 0 && !only_if_low_disk;
    if freed == 0 && outcome.failed == 0 && !warn_low && !needs_decision {
        return;
    }

    let mut lines = Vec::new();
    if freed > 0 {
        lines.push(format!("Deleted {}.", outcome.breakdown()));
    }
    if !docker.is_empty() {
        lines.push(format!("Docker: {}.", docker.join(", ")));
    }
    if needs_decision {
        lines.push(format!(
            "{} worktrees need a decision: run `{} status`.",
            outcome.needs_decision,
            env!("CARGO_PKG_NAME")
        ));
    }
    if outcome.failed > 0 {
        lines.push(format!(
            "{} deletions failed: see `journalctl --user -u {}`.",
            outcome.failed,
            env!("CARGO_PKG_NAME")
        ));
    }
    lines.push(format!("Disk: {}.", after.summary()));

    let summary = match (freed > 0, still_low) {
        (true, _) => format!("Freed {}", disk::human(freed)),
        (false, true) => "Disk space is low".to_owned(),
        (false, false) => "Disk cleanup".to_owned(),
    };
    if let Err(error) = notify::send(&summary, &lines.join("\n")) {
        eprintln!("notification failed: {error:#}");
    }
    if warn_low && let Err(error) = fs::write(&stamp, b"") {
        eprintln!("writing {}: {error:#}", stamp.display());
    }
}

fn due(stamp: &Path, every_hours: u64) -> bool {
    let Ok(last) = fs::metadata(stamp).and_then(|meta| meta.modified()) else {
        return true;
    };
    let elapsed = SystemTime::now()
        .duration_since(last)
        .unwrap_or(Duration::ZERO);
    elapsed >= Duration::from_secs(every_hours * SECONDS_PER_HOUR)
}
