// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What it costs to run a file that has just been written.

use std::path::Path;
use std::time::Duration;

/// The program copied as the probe: a real executable, small, and at a path every Unix has.
///
/// It has to be a program and not a script, because a script is not what gets
/// evaluated: it is read by an interpreter that was evaluated long ago, and a
/// machine paying minutes per new executable runs a fresh `#!/bin/sh` file in
/// milliseconds. Measured side by side on a machine in that state, a fresh
/// script cost two seconds and a fresh copy of a real program cost a hundred
/// and seventy-six. It also has to be small: the shell is tens of kilobytes,
/// where an unoptimized Rust binary is hundreds of megabytes and copying it
/// would time the disk instead.
#[cfg(unix)]
pub const PROBE_PROGRAM: &str = "/bin/sh";

/// How long a probe may take before the measurement is abandoned.
///
/// The bound is generous rather than tight: the phenomenon this measures is
/// minutes long, so a deadline that fired at the ordinary case would report
/// nothing on the machine that needed the answer. What it prevents is a probe
/// on a wedged filesystem hanging instead of reporting the slow execution it
/// was there to find.
pub const PROBE_LIMIT: Duration = Duration::from_secs(600);

/// Copies a program nobody has run from this path before, runs it twice, and hands back what each run took.
///
/// The pair is the evidence and neither number means anything alone: one slow
/// run could be a slow disk, and a slow run beside a fast run of the same file
/// cannot be anything else.
#[cfg(unix)]
#[must_use]
pub fn exec_twice(temp: &Path) -> Option<(Duration, Duration)> {
    let dir = temp.join(format!(
        "{}exec-{}",
        crate::workspace::SCRATCH_DIR_PREFIX,
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("probe");
    let copied = std::fs::copy(PROBE_PROGRAM, &path).is_ok();
    let measured = copied
        .then(|| Some((timed(&path)?, timed(&path)?)))
        .flatten();
    drop(std::fs::remove_dir_all(&dir));
    measured
}

/// Copies a program nobody has run from this path before, runs it twice, and hands back what each run took.
#[cfg(not(unix))]
#[must_use]
pub const fn exec_twice(_temp: &Path) -> Option<(Duration, Duration)> {
    None
}

/// How long one run of `path` took, or nothing when it could not be started or would not finish.
#[cfg(unix)]
fn timed(path: &Path) -> Option<Duration> {
    let mut spec =
        crate::runner::Spec::new([path.as_os_str().to_owned(), "-c".into(), "exit 0".into()]);
    spec.timeout = Some(PROBE_LIMIT);
    let result = crate::runner::run(&spec, &crate::runner::Cancel::new());
    (result.error.is_none() && !result.timed_out && result.exit_code == 0)
        .then_some(result.duration)
}
