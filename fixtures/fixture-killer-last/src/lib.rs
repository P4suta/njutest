// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A function its own unit test reaches without noticing a change to it, and an integration test notices.

mod unrelated;

pub use unrelated::unrelated;

/// Twice `n`.
pub fn double(n: u32) -> u32 {
    n * 2
}

#[cfg(test)]
mod tests {
    #[test]
    fn doubling_zero_is_zero() {
        assert_eq!(super::double(0), 0);
    }
}
