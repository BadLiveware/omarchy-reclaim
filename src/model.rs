use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Worktree,
    Cargo,
    DotNet,
    Node,
    Cache,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Worktree => "worktree",
            Kind::Cargo => "cargo",
            Kind::DotNet => "dotnet",
            Kind::Node => "node",
            Kind::Cache => "cache",
        }
    }
}

#[derive(Debug)]
pub enum Verdict {
    Delete(String),
    NeedsDecision(String),
    Keep(String),
}

impl Verdict {
    pub fn reason(&self) -> &str {
        match self {
            Verdict::Delete(reason) | Verdict::NeedsDecision(reason) | Verdict::Keep(reason) => {
                reason
            }
        }
    }
}

#[derive(Debug)]
pub enum Removal {
    Directory,
    GitWorktree { repo: PathBuf },
}

#[derive(Debug)]
pub struct Candidate {
    pub kind: Kind,
    pub path: PathBuf,
    pub idle_days: u64,
    pub verdict: Verdict,
    pub removal: Removal,
}
