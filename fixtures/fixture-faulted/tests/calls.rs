// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Asks each call once, checking some answers and not others.

use std::path::Path;

#[test]
fn the_manifest_loads() {
    assert!(fixture_faulted::load(Path::new("Cargo.toml")).is_ok());
}

#[test]
fn seven_is_a_number() {
    assert_eq!(fixture_faulted::number("7"), Ok(7));
}

#[test]
fn a_length_is_asked_and_never_checked() {
    let _length = fixture_faulted::length(Path::new("Cargo.toml"));
}

#[test]
fn ours_and_maybe_answer() {
    assert_eq!(fixture_faulted::ours(), Ok(7));
    assert_eq!(fixture_faulted::maybe(Some(3)), Some(3));
}
