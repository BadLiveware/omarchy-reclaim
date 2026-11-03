mod activity;
mod command;
mod config;
mod disk;
mod docker;
mod github;
mod model;
mod notify;
mod outputs;
mod plan;
mod procs;
mod remove;
mod run;
mod scan;
mod snapshot;
mod state;
mod status;
mod treehouse;
mod worktrees;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::config::Config;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Config file [default: $XDG_CONFIG_HOME/reclaim/config.toml]
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show what a run would delete, what needs a decision, and why
    Status {
        /// Also list everything that is kept
        #[arg(long)]
        all: bool,
    },
    /// Refresh the plan snapshot that the bar widget shows, without deleting anything
    Scan,
    /// Remove a worktree that needs a decision, accepting what it holds
    Remove {
        path: PathBuf,
        /// The reason shown for the decision. Nothing is removed if it no longer matches.
        #[arg(long)]
        accept: String,
    },
    /// Delete everything that is safe to delete
    Run {
        /// Do nothing while free space is above `low_disk_free_percent`
        #[arg(long)]
        only_if_low_disk: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::load(cli.config.as_deref())?;
    match cli.command {
        Command::Status { all } => status::show(&config, all),
        Command::Scan => status::scan(&config),
        Command::Remove { path, accept } => remove::remove(&config, &path, &accept),
        Command::Run { only_if_low_disk } => run::run(&config, only_if_low_disk),
    }
}
