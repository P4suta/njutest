// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A sweep a mutation turns on the directory the test binaries run from, so every execution after it is read against an apparatus that is no longer the one built.

use std::path::Path;

/// Removes every file in `dir` whose name says it is stale, and says how many went.
pub fn sweep(dir: &Path) -> usize {
    let mut removed = 0;
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if entry.file_name().to_string_lossy().starts_with("stale-") {
            removed += usize::from(std::fs::remove_file(entry.path()).is_ok());
        }
    }
    removed
}

/// Twice `n`.
#[must_use]
pub const fn double(n: u32) -> u32 {
    n * 2
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_sweep_of_the_running_directory_leaves_it_running() {
        let exe = std::env::current_exe().expect("the running test binary");
        let dir = exe.parent().expect("its directory");
        let _removed = super::sweep(dir);
    }

    #[test]
    fn doubling_doubles() {
        assert_eq!(super::double(3), 6);
    }
}
