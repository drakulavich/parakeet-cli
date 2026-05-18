use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;

pub fn run(out: PathBuf, max_seconds: u64) -> Result<()> {
    let summary =
        crate::record::record_default_input_to_wav(&out, Duration::from_secs(max_seconds))?;
    eprintln!(
        "Recorded {} ({} Hz, {} channel{}, {} frames)",
        summary.path.display(),
        summary.sample_rate,
        summary.channels,
        if summary.channels == 1 { "" } else { "s" },
        summary.frames,
    );
    Ok(())
}
