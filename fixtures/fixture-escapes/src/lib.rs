// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A library whose integration test starts a process that leaves the test's process group and outlives the test, as a test that starts a daemon does.

/// Twice `n`.
#[must_use]
pub fn double(n: u32) -> u32 {
    n * 2
}

#[cfg(test)]
mod tests {
    #[test]
    fn two_doubled_is_four() {
        assert_eq!(super::double(2), 4);
    }
}
