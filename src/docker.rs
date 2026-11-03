use std::process::Command;

use anyhow::Result;

use crate::command;

const RECLAIMED_PREFIX: &str = "Total reclaimed space:";

pub fn report() -> Result<String> {
    docker(&["system", "df"])
}

/// Unused volumes are throwaway test state and go at once. Images and build cache get a grace period.
pub fn reclaim(image_idle_hours: u64) -> Vec<String> {
    let until = format!("until={image_idle_hours}h");
    let steps: [(&str, Vec<&str>); 3] = [
        ("volumes", vec!["volume", "prune", "--all", "--force"]),
        (
            "images",
            vec!["image", "prune", "--all", "--force", "--filter", &until],
        ),
        (
            "build cache",
            vec!["builder", "prune", "--force", "--filter", &until],
        ),
    ];
    steps
        .into_iter()
        .map(|(what, args)| match docker(&args) {
            Ok(out) => format!("{what}: {}", reclaimed(&out)),
            Err(error) => format!("{what}: failed: {error:#}"),
        })
        .collect()
}

fn docker(args: &[&str]) -> Result<String> {
    let mut docker = Command::new("docker");
    docker.args(args);
    command::output(docker)
}

fn reclaimed(output: &str) -> String {
    output
        .lines()
        .find_map(|line| line.strip_prefix(RECLAIMED_PREFIX))
        .map(|amount| amount.trim().to_owned())
        .unwrap_or_else(|| "0B".to_owned())
}
