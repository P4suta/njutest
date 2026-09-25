// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Builds a command line from a temporary path without quoting it.

#[test]
fn a_scratch_path_is_one_word() {
    assert_eq!(environment::words(&std::env::temp_dir().join("scratch")), 1);
}
