// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A target that runs the library and asserts nothing about what it got back.

#[test]
fn doubling_runs_without_falling_over() {
    let _doubled = fixture_hollow::double(2);
}
