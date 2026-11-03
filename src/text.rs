const SAMPLE_SIZE: usize = 3;

pub fn counted(count: u64, noun: &str) -> String {
    if count == 1 {
        return format!("1 {noun}");
    }
    format!("{count} {noun}s")
}

/// The first few paths, and how many more there are.
pub fn sample(paths: &[&str]) -> String {
    let shown = paths[..paths.len().min(SAMPLE_SIZE)].join(", ");
    let rest = paths.len().saturating_sub(SAMPLE_SIZE);
    if rest == 0 {
        return shown;
    }
    format!("{shown} and {rest} more")
}
