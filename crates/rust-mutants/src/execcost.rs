// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What it costs to run a file that has just been written.

use std::path::Path;
use std::time::Duration;

/// How long a probe may take before the measurement is abandoned.
///
/// The bound is generous rather than tight: the phenomenon this measures is
/// minutes long, so a deadline that fired at the ordinary case would report
/// nothing on the machine that needed the answer. What it prevents is a probe
/// on a wedged filesystem hanging instead of reporting the slow execution it
/// was there to find.
///
/// macOS evaluates a newly written Mach-O before it may run and Windows scans
/// a newly written executable, so both are machines this measures; what varies
/// is only the name the copy has to have.
pub const PROBE_LIMIT: Duration = Duration::from_secs(600);

/// Copies a program nobody has run from this path before, runs it twice, and hands back what each run took.
///
/// The pair is the evidence and neither number means anything alone: one slow
/// run could be a slow disk, and a slow run beside a fast run of the same file
/// cannot be anything else.
///
/// # Errors
/// What stopped the measurement, which is itself a thing to be told: a silence
/// here reads as a machine that is well.
pub fn exec_twice(temp: &Path, program: &Path) -> Result<(Duration, Duration), String> {
    let dir = temp.join(format!(
        "{}exec-{}",
        crate::workspace::SCRATCH_DIR_PREFIX,
        std::process::id()
    ));
    let measured = made(&dir, program);
    drop(std::fs::remove_dir_all(&dir));
    measured
}

/// The two runs, or what stopped them, which is a thing to be told rather than a silence.
fn made(dir: &Path, program: &Path) -> Result<(Duration, Duration), String> {
    std::fs::create_dir_all(dir)
        .map_err(|why| format!("{} could not be made: {why}", dir.display()))?;
    let path = dir.join(if cfg!(windows) { "probe.exe" } else { "probe" });
    std::fs::copy(program, &path).map_err(|why| {
        format!(
            "{} could not be copied to {}: {why}",
            program.display(),
            path.display()
        )
    })?;
    Ok((timed(&path)?, timed(&path)?))
}

/// How long one run of `path` took, or nothing when it could not be started or would not finish.
fn timed(path: &Path) -> Result<Duration, String> {
    let spec = crate::runner::Spec::new(
        [path.as_os_str().to_owned(), "--version".into()],
        crate::runner::Bound::After(PROBE_LIMIT),
    );
    let result = crate::runner::run(&spec, &crate::runner::Cancel::new());
    if let Some(why) = result.error {
        return Err(format!("{} would not start: {why}", path.display()));
    }
    if result.timed_out {
        return Err(format!(
            "{} did not finish within {} seconds, which is itself the answer",
            path.display(),
            PROBE_LIMIT.as_secs()
        ));
    }
    if result.exit_code != 0 {
        return Err(format!(
            "{} exited {} rather than doing nothing successfully",
            path.display(),
            result.exit_code
        ));
    }
    Ok(result.duration)
}
