// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Counts preserve their unit and refuse arithmetic that cannot be represented exactly.

use rust_mutants::count::{Count, Mutants, Pairs, Targets, Tests};

#[test]
fn addition_and_subtraction_refuse_to_invent_a_boundary_value() {
    assert_eq!(
        Count::<Tests>::new(u64::MAX).checked_add(Count::new(1)),
        None,
        "an overflowing count is not the largest valid count"
    );
    assert_eq!(
        Count::<Tests>::new(0).checked_sub(Count::new(1)),
        None,
        "a negative count is not zero"
    );
    assert_eq!(
        Count::<Tests>::new(7).checked_sub(Count::new(2)),
        Some(Count::new(5)),
        "representable arithmetic stays exact"
    );
}

#[test]
fn converting_mutants_and_targets_to_pairs_refuses_overflow() {
    assert_eq!(
        Count::<Mutants>::new(u64::MAX).checked_against(Count::<Targets>::new(2)),
        None,
        "an overflowing product is not the largest valid pair count"
    );
    assert_eq!(
        Count::<Mutants>::new(3).checked_against(Count::<Targets>::new(4)),
        Some(Count::<Pairs>::new(12)),
        "the unit conversion is exact when the product fits"
    );
}
