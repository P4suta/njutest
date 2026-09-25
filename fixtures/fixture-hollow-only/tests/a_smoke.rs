// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A target asked first, which runs the library and asserts nothing about what it got back.

#[test]
fn signing_runs_without_falling_over() {
    let _said = fixture_hollow_only::sign(1);
}
