// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The smallest workspace the engine can open: one library with a unit test, one integration test, and a helper module only the tests compile.

#[cfg(test)]
mod testutil;

/// The larger of two numbers, spelled with a comparison a mutant can flip.
pub fn max(a: i32, b: i32) -> i32 {
    if a > b { a } else { b }
}

/// Whether `n` is even.
pub fn is_even(n: i32) -> bool {
    n % 2 == 0
}

#[cfg(test)]
mod tests {
    fn pause() {
        let Ok(text) = std::env::var("FIXTURE_SIMPLE_PAUSE_MS") else {
            return;
        };
        if let Ok(milliseconds) = text.parse::<u64>() {
            std::thread::sleep(std::time::Duration::from_millis(milliseconds));
        }
    }

    #[test]
    fn max_picks_the_larger() {
        pause();
        assert_eq!(super::max(1, 2), 2);
        assert_eq!(super::max(3, 2), 3);
        assert_eq!(super::testutil::sample(), 7);
    }
}
