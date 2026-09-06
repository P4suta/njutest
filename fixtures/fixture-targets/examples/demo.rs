// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! An example cargo also tests, which makes it a target a run has to reach.

fn main() {
    println!("{}", fixture_targets::whole(9));
}

/// The example's own claim, which `test = true` in the manifest turns into a target.
#[test]
fn the_example_agrees_with_the_library() {
    assert_eq!(fixture_targets::whole(9), 2);
}
