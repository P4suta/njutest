// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Each test counts up from whatever the previous run of it left.

use std::path::PathBuf;

fn kept(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join("fixture-durable");
    std::fs::create_dir_all(&directory).expect("a directory to keep the count in");
    directory.join(name)
}

#[test]
fn a_count_kept_in_pieces_goes_up() {
    let path = kept("pieces");
    let count = fixture_durable::load(&path).expect("the count reads");
    fixture_durable::save_in_pieces(&path, count + 1).expect("the count is kept");
    assert_eq!(fixture_durable::load(&path).expect("the count reads"), count + 1);
}

#[test]
fn a_count_kept_whole_goes_up() {
    let path = kept("whole");
    let count = fixture_durable::load(&path).expect("the count reads");
    fixture_durable::save_whole(&path, count + 1).expect("the count is kept");
    assert_eq!(fixture_durable::load(&path).expect("the count reads"), count + 1);
}

#[test]
fn a_run_that_ends_with_the_stop_status_of_its_own() {
    if std::env::var_os("RUST_MUTANTS_ACTIVE").is_some_and(|active| !active.is_empty()) {
        std::process::exit(93);
    }
}
