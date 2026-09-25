// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A branch the tests never take, behind a condition a mutation can force, so a mutant execution enters an item the baseline never did.

/// The common answer.
fn common(n: u32) -> u32 {
    n + 1
}

/// The rare answer, which only a large input reaches.
fn rare(n: u32) -> u32 {
    n * 2
}

/// Picks an answer by the size of the input.
pub fn pick(n: u32) -> u32 {
    if n > 10 { rare(n) } else { common(n) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_small_input_takes_the_common_answer() {
        assert_eq!(super::pick(1), 2);
    }
}
