// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! An integration target, so the baseline has two binaries to run and two
//! sets of reached regions to tell apart.

#[test]
fn doubling_is_addition_twice() {
    assert_eq!(fixture_baseline::double(21), 42);
}
