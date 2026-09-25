// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One test that fails at once when the answer about nothing is wrong, and one that then waits forever without running any of the library again.

#[test]
fn a_says_nothing_is_not_ready() {
    assert!(!fixture_balanced_fails_then_hangs::ready(0));
}

#[test]
fn b_waits_while_nothing_is_ready() {
    if fixture_balanced_fails_then_hangs::ready(0) {
        loop {
            std::thread::park();
        }
    }
}
