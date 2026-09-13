// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Two ways a test process can end other than by failing an assertion.

/// `n`, where a caller asked for something in range.
///
/// The bound is where it is so that a mutation of it is noticed by the
/// `#[should_panic]` test and by nothing else: ten is out of range, and a
/// mutation that moves the bound past ten is one only a test that asks for a
/// panic can see.
///
/// # Panics
/// When `n` is ten or more.
#[must_use]
pub fn in_range(n: u32) -> u32 {
    if n >= 10 {
        panic!("out of range");
    }
    n
}

/// `n`, where a caller asked for a capacity at all.
///
/// A capacity of zero is a caller that cannot be answered, and there is
/// nothing to return, so the process stops. It is what a mutation has to make
/// happen for a run to find out whether a process that aborts reads as a kill
/// on every platform.
#[must_use]
pub fn capacity(n: usize) -> usize {
    if n == 0 {
        std::process::abort()
    }
    n
}

#[cfg(test)]
mod tests {
    use super::{capacity, in_range};

    #[test]
    fn a_value_in_range_is_itself() {
        assert_eq!(in_range(4), 4);
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn a_value_at_the_bound_panics() {
        let _unreached = in_range(10);
    }

    #[test]
    fn a_value_below_the_bound_is_itself() {
        assert_eq!(in_range(9), 9);
    }

    #[test]
    fn a_capacity_is_itself() {
        assert_eq!(capacity(1), 1);
        assert_eq!(capacity(4), 4);
    }
}
