// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One test that fails at once when the answer is wrong, and one that then waits forever without running any of the library again.

#[test]
fn a_says_the_answer_is_ready() {
    assert!(fixture_fails_then_hangs::ready(1));
}

#[test]
fn b_waits_until_the_answer_is_ready() {
    if !fixture_fails_then_hangs::ready(1) {
        loop {
            std::thread::park();
        }
    }
}
