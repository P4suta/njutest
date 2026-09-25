// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A condition of two calls, which a run cannot evaluate twice, tested only where its left operand is false.

/// Whether one count is over the limit.
fn over(count: u32) -> bool {
    count > 9
}

/// Whether either count is over the limit.
pub fn either_over(left: u32, right: u32) -> bool {
    if over(left) || over(right) {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::either_over;

    #[test]
    fn the_right_count_alone_decides_when_the_left_is_under() {
        assert!(either_over(0, 10));
        assert!(!either_over(0, 0));
    }
}
