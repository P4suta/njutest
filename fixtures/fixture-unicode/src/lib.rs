// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Non-ASCII before the code that matters, so that "which column?" has one answer and a test can hold both tools to it.

/// The identifiers and the comment before the comparison are multi-byte, so its byte column and its character column differ.
pub fn 大きい方(α: i32, β: i32) -> i32 { let γ = α; if γ > β { γ } else { β } }

/// Nothing calls this, so its region is instrumented and uncovered.
pub fn 使われない(δ: i32) -> i32 {
    δ * 2
}

#[cfg(test)]
mod tests {
    #[test]
    fn 大きい方を選ぶ() {
        assert_eq!(super::大きい方(1, 2), 2);
    }
}
