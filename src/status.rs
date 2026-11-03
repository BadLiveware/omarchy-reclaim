use anyhow::Result;

use crate::config::{self, Config};
use crate::disk::{self, Usage};
use crate::model::Verdict;
use crate::plan::{self, Measured, Plan};
use crate::snapshot::{self, PlanSnapshot};
use crate::{docker, state, treehouse};

const LISTED_PER_GROUP: usize = 25;

pub fn show(config: &Config, all: bool) -> Result<()> {
    println!("Disk: {}", Usage::of(&config::home()?)?.summary());
    let measured = plan::measure(Plan::build(config)?.candidates, all);
    let limit = list_limit(all);
    print_group("Delete", &measured, limit, |verdict| {
        matches!(verdict, Verdict::Delete(_))
    });
    print_group("Needs a decision", &measured, limit, |verdict| {
        matches!(verdict, Verdict::NeedsDecision(_))
    });
    print_group("Keep", &measured, keep_limit(all), |verdict| {
        matches!(verdict, Verdict::Keep(_))
    });
    if config.treehouse {
        print_section("Treehouse prune (dry run)", treehouse::report());
    }
    if config.docker {
        print_section("Docker", docker::report());
    }
    Ok(())
}

/// Refreshes the plan snapshot without deleting anything.
pub fn scan(config: &Config) -> Result<()> {
    let dir = state::dir()?;
    let Some(_lock) = state::lock(&dir)? else {
        println!("another scan or run is active");
        return Ok(());
    };
    let measured = plan::measure(Plan::build(config)?.candidates, false);
    let snapshot = PlanSnapshot::new(config, &measured)?;
    state::write_json(&dir, snapshot::PLAN_FILE, &snapshot)?;
    println!(
        "{} reclaimable, {} worktrees need a decision",
        disk::human(snapshot.reclaimable_bytes),
        snapshot.needs_decision.len()
    );
    Ok(())
}

fn list_limit(all: bool) -> usize {
    if all {
        return usize::MAX;
    }
    LISTED_PER_GROUP
}

fn keep_limit(all: bool) -> usize {
    if all {
        return usize::MAX;
    }
    0
}

fn print_group(
    title: &str,
    measured: &[Measured],
    limit: usize,
    belongs: impl Fn(&Verdict) -> bool,
) {
    let mut group: Vec<&Measured> = measured
        .iter()
        .filter(|item| belongs(&item.candidate.verdict))
        .collect();
    group.sort_by_key(|item| std::cmp::Reverse(item.bytes));
    let total: Option<u64> = group.iter().map(|item| item.bytes).sum();
    let total = total
        .map(disk::human)
        .unwrap_or_else(|| "size not measured".to_owned());
    println!("\n{title}: {} items, {total}", group.len());
    for item in group.iter().take(limit) {
        let candidate = &item.candidate;
        let size = item.bytes.map(disk::human).unwrap_or_default();
        println!(
            "  {:<8} {:>10} {:>4}d  {}  ({})",
            candidate.kind.label(),
            size,
            candidate.idle_days,
            config::display(&candidate.path),
            candidate.verdict.reason()
        );
    }
    let hidden = group.len().saturating_sub(limit);
    if hidden > 0 && limit > 0 {
        println!("  … and {hidden} more (--all lists everything)");
    }
}

fn print_section(title: &str, output: Result<String>) {
    println!("\n{title}:");
    match output {
        Ok(text) => text.lines().for_each(|line| println!("  {line}")),
        Err(error) => println!("  unavailable: {error:#}"),
    }
}
