// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A suite whose reach depends on whether an earlier process of the same run already ran it.

/// What the suite calls when it is the first process of a run to look.
#[must_use]
pub fn first_visit(n: u32) -> u32 {
    n + 1
}

/// What the suite calls when an earlier process of the same run already looked.
#[must_use]
pub fn return_visit(n: u32) -> u32 {
    n * 2
}

/// What the suite checks either way.
#[must_use]
pub fn sum(a: u32, b: u32) -> u32 {
    a + b
}

#[cfg(test)]
mod tests {
    /// The prefix of the directory a mutation run gives every process of one run, and removes when it ends.
    const RUN_SCRATCH: &str = "rm-scratch-";

    /// Whether an earlier process of this run already looked, leaving a mark for the next in the run scratch above its temporary directory, where every process of one run can see it and no process of another run can.
    fn visited() -> bool {
        let temporary = std::env::temp_dir();
        let Some(run) = temporary.ancestors().find(|dir| {
            dir.file_name()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|name| name.starts_with(RUN_SCRATCH))
        }) else {
            return false;
        };
        let mark = run.join("fixture-drifts-visited");
        if mark.exists() {
            return true;
        }
        std::fs::write(&mark, b"").is_err()
    }

    #[test]
    fn adds() {
        if visited() {
            std::hint::black_box(super::return_visit(2));
        } else {
            std::hint::black_box(super::first_visit(1));
        }
        assert_eq!(super::sum(2, 3), 5);
    }
}
