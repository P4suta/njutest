// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Assumes the language is not Turkish, whose dotless i breaks case folding.

#[test]
fn the_language_is_not_turkish() {
    assert!(!environment::language().starts_with("tr"));
}
