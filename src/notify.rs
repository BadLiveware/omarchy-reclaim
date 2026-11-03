use std::process::Command;

use anyhow::Result;

use crate::command;

pub fn send(summary: &str, body: &str) -> Result<()> {
    let mut notify = Command::new("notify-send");
    notify
        .args(["--app-name", env!("CARGO_PKG_NAME")])
        .arg(summary)
        .arg(body);
    command::output(notify)?;
    Ok(())
}
