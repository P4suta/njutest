// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Three mutations at `opt-level = 2`: one the compiler renders identically, one it renders differently, and one in code no test calls.

/// `n`, written the long way round.
pub fn unchanged(n: i32) -> i32 {
    n + 0
}

/// Twice `n`.
pub fn doubled(n: i32) -> i32 {
    n * 2
}

/// Half of `n`. Nothing calls it, so the linker drops it and a build without it is the same file.
pub fn halved(n: i32) -> i32 {
    n / 2
}

#[cfg(test)]
mod tests {
    #[test]
    fn what_the_tests_call() {
        assert_eq!(super::unchanged(3), 3);
        assert_eq!(super::doubled(3), 6);
    }
}
