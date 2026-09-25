// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Assumes the home directory is the one a person uses.

#[test]
fn the_home_directory_holds_something() {
    assert!(environment::home_holds_something());
}
